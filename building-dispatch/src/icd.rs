use std::str::FromStr;

pub const MAX_ELEVATORS: usize = 3;
pub const MAX_FLOORS: usize = 5;

impl FromStr for Request {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let words = s.split_whitespace().collect::<Vec<&str>>();

        // NOTE: Doesn't do bounds checking because this is a "dumb" deserializer
        fn get(words: &[&str]) -> Option<Request> {
            match words {
                ["elevator", elevator, "get"] => {
                    let elevator = elevator.parse::<usize>().ok()?;
                    Some(Request::Elevator(ElevatorReq::GetFloor { elevator_idx: elevator }))
                }
                ["elevator", elevator, "goto", floor] => {
                    let elevator = elevator.parse::<usize>().ok()?;
                    let floor = floor.parse::<usize>().ok()?;
                    Some(Request::Elevator(ElevatorReq::GoToFloor { elevator_idx: elevator, floor }))
                }
                ["lights", floor, "get"] => {
                    let floor = floor.parse::<usize>().ok()?;
                    Some(Request::Lighting(LightingReq::GetLevel { floor }))
                }
                ["lights", floor, "set", brightness] => {
                    let brightness = brightness.parse::<u8>().ok()?;
                    let floor = floor.parse::<usize>().ok()?;
                    Some(Request::Lighting(LightingReq::SetLevel { brightness, floor }))
                }
                _ => None
            }
        }

        get(&words).ok_or_else(|| format!("Invalid: '{}'", s.trim()))
    }
}

#[derive(Debug, PartialEq)]
pub enum Request {
    Elevator(ElevatorReq),
    Lighting(LightingReq),
}

#[derive(Debug, PartialEq)]
pub enum ElevatorReq {
    GetFloor {
        elevator_idx: usize,
    },
    GoToFloor {
        elevator_idx: usize,
        floor: usize,
    },
}

#[derive(Debug, PartialEq)]
pub enum ElevatorResp {
    AtFloor {
        elevator_idx: usize,
        floor: usize,
    },
    MovedToFloor {
        elevator_idx: usize,
        floor: usize,
    }
}

#[derive(Debug, PartialEq)]
pub enum ElevatorErr {
    InvalidElevator(usize),
    InvalidFloor(usize),
    ElevatorBusy,
}

#[derive(Debug, PartialEq)]
pub enum LightingReq {
    GetLevel {
        floor: usize,
    },
    SetLevel {
        brightness: u8,
        floor: usize,
    },
}

#[derive(Debug, PartialEq)]
pub enum LightingResp {
    AtBrightness {
        brightness: u8,
        floor: usize,
    }
}

#[derive(Debug, PartialEq)]
pub enum LightingErr {
    InvalidFloor(usize),
}

#[derive(Debug, PartialEq)]
pub enum Response {
    Elevator(Result<ElevatorResp, ElevatorErr>),
    Lighting(Result<LightingResp, LightingErr>),
    Bad
}
