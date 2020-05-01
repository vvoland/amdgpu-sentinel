use std::ops::RangeInclusive;
use std::vec::Vec;

use crate::polaris_gpu::Part;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct PolarisGpuTable {
    voltage_range: RangeInclusive::<u32>,
    sclk_range: RangeInclusive::<u32>,
    mclk_range: RangeInclusive::<u32>,
    memory_states: Vec::<PolarisGpuState>,
    core_states: Vec::<PolarisGpuState>
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct PolarisGpuState {
    pub clock: u32,
    pub voltage: u32
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateInvalidReason {
    VoltageNotInRange,
    ClockNotInRange,
    InvalidIndex
}

impl std::fmt::Display for PolarisGpuState {

    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{} MHz @ {} mV", self.clock, self.voltage)
    }

}

impl std::fmt::Display for PolarisGpuTable {

    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let memory_states = self.memory_states.iter()
            .map(|state| state.to_string())
            .collect::<Vec::<String>>()
            .join("\r\n");

        let core_states = self.core_states.iter()
            .map(|state| state.to_string())
            .collect::<Vec::<String>>()
            .join("\r\n");

        write!(f, "Memory states:\r\n{}\r\nCore states:\r\n{}", memory_states, core_states)
    }

}


impl PolarisGpuTable {

    pub fn voltage_range(&self) -> RangeInclusive::<u32> {
        RangeInclusive::new(*self.voltage_range.start(), *self.voltage_range.end())
    }

    pub fn clock_range(&self, part: Part) -> RangeInclusive::<u32> {
        let range = match part {
            Part::Core => &self.sclk_range,
            Part::Memory => &self.mclk_range
        };
        RangeInclusive::new(*range.start(), *range.end())
    }

    pub fn states(&self, part: Part) -> &Vec::<PolarisGpuState> {
        match part {
            Part::Core => &self.core_states,
            Part::Memory => &self.memory_states
        }
    }

    pub fn get_state(&self, part: Part, index: usize) -> Option<PolarisGpuState> {
        match self.states(part).get(index) {
            Some(state) => Some(state.clone()),
            None => None
        }
    }

    pub fn set_state(&mut self, part: Part, index: usize, state: PolarisGpuState) -> Result<(), StateInvalidReason> {
        match self.validate_state(part, state) {
            Ok(_) => {
                let vec = match part {
                    Part::Core => &mut self.core_states,
                    Part::Memory => &mut self.memory_states
                };

                if index < vec.len() {
                    vec[index] = state;
                    Ok(())
                } else { 
                    Err(StateInvalidReason::InvalidIndex)
                }
            }
            Err(reason) => Err(reason)
        }
    }

    pub fn validate_state<'a>(&self, part: Part, state: PolarisGpuState) -> Result<(), StateInvalidReason> {
        let clock_range = match part {
            Part::Core => &self.sclk_range,
            Part::Memory => &self.mclk_range
        };

        if !self.voltage_range.contains(&state.voltage) {
            Err(StateInvalidReason::VoltageNotInRange)
        } else if !clock_range.contains(&state.clock) {
            Err(StateInvalidReason::ClockNotInRange)
        } else {
            Ok(())
        }
    }

    fn parse_unit<'a>(data: &'a str, unit: &'static str) -> Option<u32> {
        if data.ends_with(unit) {
            match data.replace(unit, "").parse::<u32>() {
                Ok(value) => Some(value),
                Err(_) => None
            }
        } else {
            None
        }
    }

    pub fn parse<'a>(data: &'a str) -> PolarisGpuTable {
        Self::try_parse(data).expect("Failed to parse PolarisGpuTable")
    }

    pub fn try_parse<'a>(data: &'a str) -> Option<PolarisGpuTable> {
        let mut voltage_range: Option<RangeInclusive::<u32>> = None;
        let mut sclk_range: Option<RangeInclusive::<u32>> = None;
        let mut mclk_range: Option<RangeInclusive::<u32>> = None;
        let mut core_states: Vec::<PolarisGpuState> = vec![];
        let mut memory_states: Vec::<PolarisGpuState> = vec![];

        enum ParserState {
            Initial,
            Core,
            Memory,
            Ranges
        };
        let mut state = ParserState::Initial;
        for line in data.split("\n") {
            let mut semicolon_split = line.trim().split(":");

            let prefix = semicolon_split.next().unwrap();
            let maybe_data = semicolon_split.next();

            let data = match maybe_data {
                Some(d) => d.trim(),
                None => ""
            };

            if data != "" {
                let mut data_split = data.split_whitespace();
                match state {
                    ParserState::Initial => panic!("Don't know what I'm parsing"),
                    ParserState::Core | ParserState::Memory => {
                        let clock_str = data_split.next().expect("No clock");
                        let voltage_str = data_split.next().expect("No voltage");

                        let clock = Self::parse_unit(&clock_str, "MHz").expect("Invalid clock value");
                        let voltage = Self::parse_unit(&voltage_str, "mV").expect("Invalid voltage value");

                        let states = match state {
                            ParserState::Core => &mut core_states,
                            ParserState::Memory => &mut memory_states,
                            _ => unreachable!("Won't happen!")
                        };
                        states.push(PolarisGpuState { clock, voltage });
                    },
                    ParserState::Ranges => {
                        let lower_str = data_split.next().expect("No lower voltage bound");
                        let upper_str = data_split.next().expect("No upper voltage bound");

                        let unit = match prefix {
                            "SCLK" | "MCLK" => "MHz",
                            "VDDC" => "mV",
                            _ => panic!("Unknown range target")
                        };

                        let lower = Self::parse_unit(&lower_str, unit).expect("Invalid lower range bound");
                        let upper = Self::parse_unit(&upper_str, unit).expect("Invalid upper range bound");

                        let range = RangeInclusive::new(lower, upper);

                        match prefix {
                            "SCLK" => sclk_range = Some(range),
                            "MCLK" => mclk_range = Some(range),
                            "VDDC" => voltage_range = Some(range),
                            _ => panic!("Unknown range target")
                        };
                    }
                }
            } else {
                match prefix.trim() {
                    "OD_SCLK" => state = ParserState::Core,
                    "OD_MCLK" => state = ParserState::Memory,
                    "OD_RANGE" => state = ParserState::Ranges,
                    "" => continue,
                    _ => panic!(format!("Unknown prefix {}", prefix))
                }
            }
        }

        if voltage_range.is_some() && sclk_range.is_some() && mclk_range.is_some() &&
            memory_states.len() > 0 && core_states.len() > 0
        { 
            Some(PolarisGpuTable { 
                voltage_range:  voltage_range.unwrap(),
                sclk_range:  sclk_range.unwrap(),
                mclk_range:  mclk_range.unwrap(),
                memory_states,
                core_states})
        } else {
            None
        }
    }


}

mod tests {

    #[test]
    fn parses_pstate_table() {
        use super::*;

        let data = "\n\
        OD_SCLK:\n\
        0:        300MHz        750mV\n\
        1:        588MHz        765mV\n\
        2:        952MHz        931mV\n\
        3:       1041MHz       1006mV\n\
        4:       1106MHz       1068mV\n\
        5:       1168MHz       1131mV\n\
        6:       1209MHz       1150mV\n\
        7:       1244MHz       1150mV\n\
        OD_MCLK:\n\
        0:        300MHz        750mV\n\
        1:       1000MHz        800mV\n\
        2:       1500MHz        900mV\n\
        OD_RANGE:\n\
        SCLK:     300MHz       2000MHz\n\
        MCLK:     300MHz       2250MHz\n\
        VDDC:     750mV        1150mV\n\
        ";

        let table = PolarisGpuTable::parse(&data);
        assert_eq!(table.voltage_range(), RangeInclusive::new(750, 1150));
        assert_eq!(table.clock_range(Part::Core), RangeInclusive::new(300, 2000));
        assert_eq!(table.clock_range(Part::Memory), RangeInclusive::new(300, 2250));
        let states = table.states(Part::Core);
        assert_eq!(states[0].clock, 300);
        assert_eq!(states[0].voltage, 750);
        assert_eq!(states[1].clock, 588);
        assert_eq!(states[1].voltage, 765);
        assert_eq!(states[2].clock, 952);
        assert_eq!(states[2].voltage, 931);
        assert_eq!(states[3].clock, 1041);
        assert_eq!(states[3].voltage, 1006);
        assert_eq!(states[4].clock, 1106);
        assert_eq!(states[4].voltage, 1068);
        assert_eq!(states[5].clock, 1168);
        assert_eq!(states[5].voltage, 1131);
        assert_eq!(states[6].clock, 1209);
        assert_eq!(states[6].voltage, 1150);
        assert_eq!(states[7].clock, 1244);
        assert_eq!(states[7].voltage, 1150);
        let mstates = table.states(Part::Memory);
        assert_eq!(mstates[0].clock, 300);
        assert_eq!(mstates[0].voltage, 750);
        assert_eq!(mstates[1].clock, 1000);
        assert_eq!(mstates[1].voltage, 800);
        assert_eq!(mstates[2].clock, 1500);
        assert_eq!(mstates[2].voltage, 900);
    }
}
