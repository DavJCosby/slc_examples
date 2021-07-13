use colortemp::temp_to_rgb;
use rand::{prelude::ThreadRng, Rng};
use std::{collections::HashMap, sync::Arc, thread, time::Instant};

use slc::prelude::*;

const SPAWN_RADIUS: f32 = 0.75;
const MIN_TEMP: i64 = 2950;
const MAX_TEMP: i64 = 6350;

const UPDATE_TIMING: f32 = 1.0 / 960.0;

struct Star {
    start_position: Point,
    position: Point,
    color: Color,
    birthday: Instant,
}

struct StarController {
    stars: Vec<Star>,
}

pub struct Warpspeed {
    movement_dir: Vector2D,
    movement_speed: f32,
    stop: bool,
}

impl Warpspeed {
    pub fn new(movement_dir: Vector2D, movement_speed: f32) -> Self {
        Warpspeed {
            movement_dir,
            movement_speed,
            stop: false,
        }
    }
}

impl StarController {
    fn add_star(&mut self, spawn_center: Point, rng: &mut ThreadRng) {
        let position = (
            spawn_center.0 + (rng.gen::<f32>() - 0.5) * SPAWN_RADIUS,
            spawn_center.1 + (rng.gen::<f32>() - 0.5) * SPAWN_RADIUS,
        );

        let brightness = rng.gen_range(0.025..0.4);

        let kelvin = rng.gen_range(MIN_TEMP..MAX_TEMP);
        let color64 = temp_to_rgb(kelvin);
        let birthday = Instant::now();

        let star = Star {
            start_position: position,
            position,
            color: (
                (color64.r * brightness) as u8,
                (color64.g * brightness) as u8,
                (color64.b * brightness) as u8,
            ),
            birthday,
        };
        self.stars.push(star);
    }

    fn update_stars(&mut self, movement_dir: Vector2D, movement_speed: f32) {
        let star_vel = (
            -movement_dir.0 * movement_speed,
            -movement_dir.1 * movement_speed,
        );
        for mut star in &mut self.stars {
            let elapsed = star.birthday.elapsed().as_secs_f32();
            let new_position = (
                star.start_position.0 + star_vel.0 * elapsed,
                star.start_position.1 + star_vel.1 * elapsed,
            );
            star.position = new_position;
        }
    }

    fn render_stars(&self, input_handle: &RoomControllerInputHandle) {
        let mut write = input_handle.write().unwrap();

        for led in 0..write.room_data.leds().len() {
            let col = write.room_data.leds()[led];
            write.set(
                led,
                (
                    (col.0 as f32 * 0.925) as u8,
                    (col.1 as f32 * 0.935) as u8,
                    (col.2 as f32 * 0.94) as u8, //  artificial blueshift
                ),
            );
        }
        drop(write);

        let read = input_handle.read().unwrap();

        let view_pos = read.room_data.view_pos();

        let mut affected_leds: HashMap<usize, (f32, f32, f32)> = HashMap::new();
        for star in &self.stars {
            let dir = (star.position.0 - view_pos.0, star.position.1 - view_pos.1);
            if let Some((id, occupancy)) = read.get_led_at_room_dir(dir) {
                let div = 1.0 / 255.0;
                let colorf32 = (
                    star.color.0 as f32 * div,
                    star.color.1 as f32 * div,
                    star.color.2 as f32 * div,
                );

                let dist_squared = ((view_pos.0 - star.position.0).powi(2)
                    + (view_pos.1 - star.position.1).powi(2))
                .max(0.2);
                // distance squared law
                let div = 1.0 / dist_squared;
                let mut colorf32_0 = (
                    (colorf32.0 * div) * occupancy,
                    (colorf32.1 * div) * occupancy,
                    (colorf32.2 * div) * occupancy,
                );

                if affected_leds.contains_key(&id) {
                    let old = affected_leds[&id];
                    colorf32_0 = (
                        colorf32_0.0 + old.0,
                        colorf32_0.1 + old.1,
                        colorf32_0.2 + old.2,
                    );
                }

                // only apply anti-aliasing to distant stars...
                if id + 1 < read.room_data.leds().len() && dist_squared > 0.2 {
                    let next_occ = 1.0 - occupancy;
                    let mut colorf32_1 = (
                        (colorf32.0 * div) * next_occ,
                        (colorf32.1 * div) * next_occ,
                        (colorf32.2 * div) * next_occ,
                    );

                    if affected_leds.contains_key(&(id + 1)) {
                        let old = affected_leds[&(id + 1)];
                        colorf32_1 = (
                            colorf32_1.0 + old.0,
                            colorf32_1.1 + old.1,
                            colorf32_1.2 + old.2,
                        );
                    }
                    affected_leds.insert(id + 1, colorf32_1);
                } else {
                    let div = 1.0 / occupancy;
                    colorf32_0 = (colorf32_0.0 * div, colorf32_0.1 * div, colorf32_0.2 * div);
                }
                affected_leds.insert(id, colorf32_0);
            }
        }

        drop(read);

        let mut write = input_handle.write().unwrap();
        // reinhard tonemapping
        for (id, colorf32) in affected_leds {
            let tonemapped = (
                colorf32.0 / (colorf32.0 + 1.0),
                colorf32.1 / (colorf32.1 + 1.0),
                colorf32.2 / (colorf32.2 + 1.0),
            );

            let tonemappedu8 = (
                (tonemapped.0 * 255.0) as u8,
                (tonemapped.1 * 255.0) as u8,
                (tonemapped.2 * 255.0) as u8,
            );
            write.set(id, tonemappedu8);
        }

        drop(write);
    }
}

impl InputDevice for Warpspeed {
    fn start(&self, input_handle: RoomControllerInputHandle) {
        let mut star_contoller = StarController { stars: vec![] };
        let movement_dir = self.movement_dir;
        let movement_speed = self.movement_speed;
        let stop = Arc::new(self.stop);
        thread::spawn(move || {
            let read = input_handle.read().unwrap();
            let spawn_center = (
                read.room_data.view_pos().0 + movement_dir.0 * 7.5,
                read.room_data.view_pos().1 + movement_dir.1 * 7.5,
            );
            drop(read);

            let mut rng = rand::thread_rng();

            let start = Instant::now();
            let mut last = 0.0;
            let mut next_spawn = 0.0;

            while !*stop {
                let duration = start.elapsed().as_secs_f32();
                if duration - last < UPDATE_TIMING {
                    continue;
                }

                if duration > next_spawn {
                    star_contoller.add_star(spawn_center, &mut rng);
                    next_spawn = duration + rng.gen_range(0.075..0.125);
                }

                star_contoller
                    .stars
                    .retain(|star| star.birthday.elapsed().as_secs_f32() < 30.0);

                star_contoller.update_stars(movement_dir, movement_speed);
                star_contoller.render_stars(&input_handle);
                last = duration;
            }
        });
    }

    fn stop(&mut self) {
        self.stop = true;
    }
}
