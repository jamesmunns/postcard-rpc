use std::{io::Write, thread::sleep, time::Duration};

use icd::{Response, MAX_ELEVATORS, MAX_FLOORS, ElevatorReq, ElevatorResp, LightingReq, LightingResp, LightingErr, ElevatorErr};

use crate::icd::Request;

mod icd;

fn main() {
    let mut buf = String::new();
    let mut state = Building::new();

    loop {
        buf.clear();
        print!("> ");
        std::io::stdout().flush().ok();
        std::io::stdin().read_line(&mut buf).unwrap();
        match buf.parse::<Request>() {
            Ok(req) => {
                println!("-> {:?}", req);
                let resp = state.operate(req);
                println!("<- {:?}", resp);
            }
            Err(e) => {
                println!("ERR: {}", e);
            }
        }
    }
}

struct Building {
    elevators: [usize; MAX_ELEVATORS],
    lighting: [u8; MAX_FLOORS],
}

impl Building {
    fn new() -> Self {
        Self {
            // all elevators start at ground level
            elevators: [0; MAX_ELEVATORS],
            // all lights start at half
            lighting: [u8::MAX / 2; MAX_FLOORS],
        }
    }

    fn operate(&mut self, req: Request) -> Response {
        match req {
            Request::Elevator(r) => Response::Elevator(self.elevator_operate(r)),
            Request::Lighting(r) => Response::Lighting(self.lighting_operate(r)),
        }
    }

    fn elevator_operate(&mut self, req: ElevatorReq) -> Result<ElevatorResp, ElevatorErr> {
        match req {
            ElevatorReq::GetFloor { elevator_idx } => {
                let Some(floor) = self.elevators.get(elevator_idx) else {
                    return Err(ElevatorErr::InvalidElevator(elevator_idx));
                };
                Ok(ElevatorResp::AtFloor { elevator_idx, floor: *floor })
            },
            ElevatorReq::GoToFloor { elevator_idx, floor } => {
                let Some(elev_floor) = self.elevators.get_mut(elevator_idx) else {
                    return Err(ElevatorErr::InvalidElevator(elevator_idx));
                };
                if floor >= MAX_FLOORS {
                    return Err(ElevatorErr::InvalidFloor(floor));
                }
                let diff = floor.abs_diff(*elev_floor);
                // elevators take time to move
                sleep(3 * Duration::from_secs(diff as u64));
                *elev_floor = floor;
                Ok(ElevatorResp::MovedToFloor { elevator_idx, floor })
            },
        }
    }

    fn lighting_operate(&mut self, req: LightingReq) -> Result<LightingResp, LightingErr> {
        match req {
            LightingReq::GetLevel { floor } => {
                let Some(light) = self.lighting.get(floor) else {
                    return Err(LightingErr::InvalidFloor(floor));
                };
                Ok(LightingResp::AtBrightness { brightness: *light, floor })
            },
            LightingReq::SetLevel { brightness, floor } => {
                let Some(light) = self.lighting.get_mut(floor) else {
                    return Err(LightingErr::InvalidFloor(floor));
                };
                *light = brightness;
                Ok(LightingResp::AtBrightness { brightness, floor })
            },
        }
    }
}
