use std::convert::TryInto;
use std::ops::Div;
use std::fmt::Display;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::path::Path;
use std::fs::File;
use std::io::prelude::*;
use std::{thread, time};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

extern crate signal_hook;
extern crate num;

mod clamped_percentage;
use clamped_percentage::*;
mod stats;
use stats::*;
mod circular_buffer;
use circular_buffer::CircularBuffer;

pub struct PolarisGpu<'a> {
    name: &'a str,
    sysfs_dir: &'a Path,
}

pub enum TemperatureSensor {
    Edge,
    Junction,
    Memory
}

#[derive(PartialEq, Clone, Copy)]
pub enum FanMode {
    Auto,
    Manual
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PerformanceLevel {
    Auto,
    Low,
    High,
    Manual,
    ProfileStandard,
    ProfileMinSclk,
    ProfileMinMclk,
    ProfilePeak
}

impl Display for PerformanceLevel {

    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> { 
        std::fmt::Debug::fmt(self, f)

    }
}

#[derive(Debug)]
pub enum PcieLevel {
    Gen1,
    Gen3
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverclockError {
    Disabled,
    VoltageNotInRange(RangeInclusive::<u32>),
    ClockNotInRange(RangeInclusive::<u32>),
    Unknown(std::io::ErrorKind)
}

impl<'a> PolarisGpu<'_> {

    pub fn usage(&self) -> ClampedPercentage {
        let percent: u32 = Self::parse_string_from_file(&self.sysfs_dir.join("gpu_busy_percent"));
        ClampedPercentage::new(percent)
    }

    pub fn temperature(&self) -> f32 {
        self.read_sensor(TemperatureSensor::Edge).expect("GPU has no temperature sensor!")
    }

    pub fn power_limit(&self) -> f32 {
        let wattage: f32 = Self::parse_string_from_file(&self.sysfs_dir.join("hwmon/hwmon0/power1_cap"));
        wattage / Self::WATTAGE_DIVISOR
    }

    const WATTAGE_DIVISOR: f32 = 1000000f32;
    fn to_real_wattage(value: f32) -> u32 { (value * Self::WATTAGE_DIVISOR) as u32 }

    pub fn power_limit_range(&self) -> RangeInclusive<f32> {
        let min: f32 = Self::parse_string_from_file(&self.sysfs_dir.join("hwmon/hwmon0/power1_cap_min"));
        let max: f32 = Self::parse_string_from_file(&self.sysfs_dir.join("hwmon/hwmon0/power1_cap_max"));

        let divisor = Self::WATTAGE_DIVISOR;

        RangeInclusive::new(min / divisor, max / divisor)
    }

    pub fn set_power_limit(&self, wattage: f32) -> () {
        let path: PathBuf = self.sysfs_dir.join("hwmon/hwmon0/power1_cap");
        let range: RangeInclusive<f32> = self.power_limit_range();

        if range.contains(&wattage) {
            let real_value: u32 = Self::to_real_wattage(wattage);
            Self::write(path, &real_value.to_string());
        } else {
            panic!("Wattage must be in range [{}, {}]", range.start(), range.end());
        }
    }

    const FAN_MODE_FILE: &'static str = "hwmon/hwmon0/pwm1_enable";
    const FAN_PWM_FILE:  &'static str = "hwmon/hwmon0/pwm1";


    pub fn fan_mode(&self) -> FanMode {
        let mode: u32 = Self::parse_string_from_file(&self.sysfs_dir.join(Self::FAN_MODE_FILE));

        match mode {
            1 => FanMode::Manual,
            2 => FanMode::Auto,
            _ => panic!("Unknown fan mode")
        }
    }

    pub fn set_fan_mode(&self, mode: FanMode) {
        let value = match mode {
            FanMode::Manual => 1,
            FanMode::Auto => 2
        };
        let path = self.sysfs_dir.join(Self::FAN_MODE_FILE);

        Self::write(path, &value.to_string());
    }

    pub fn fan_speed(&self) -> ClampedPercentage {
        let path: PathBuf = self.sysfs_dir.join(Self::FAN_PWM_FILE);
        let value: u8 = Self::parse_string_from_file(&path);

        ClampedPercentage::new(value as f32 / 255f32 * 100f32)
    }

    pub fn set_fan_speed(&self, speed: ClampedPercentage) {
        let value: u8 = (speed.0 * 255f64 / 100f64) as u8;
        let path: PathBuf = self.sysfs_dir.join(Self::FAN_PWM_FILE);

        Self::write(path, &value.to_string());
    }

    const PSTATE_MEMORY_FILE: &'static str = "pp_dpm_mclk";
    const PSTATE_CORE_FILE: &'static str = "pp_dpm_sclk";

    pub fn pstate_memory(&self) -> u32 {
        let path: PathBuf = self.sysfs_dir.join(Self::PSTATE_MEMORY_FILE);
        let data: String = Self::read_string_from_file(&path);

        Self::parse_current_pstate(data)
    }

    pub fn set_pstate_memory(&self, state: u32) {
        let path: PathBuf = self.sysfs_dir.join(Self::PSTATE_MEMORY_FILE);
        Self::write(path, &state.to_string());
    }
 
    pub fn pstate_core(&self) -> u32 {
        let path: PathBuf = self.sysfs_dir.join(Self::PSTATE_CORE_FILE);
        let data: String = Self::read_string_from_file(&path);

        Self::parse_current_pstate(data)
    }

    pub fn set_pstate_core(&self, state: u32) {
        let path: PathBuf = self.sysfs_dir.join(Self::PSTATE_CORE_FILE);
        Self::write(path, &state.to_string());
    }

    pub fn modify_pstate_core(&self, state: u32, clock: u32, voltage: u32) -> Result<(), OverclockError> {
        let cmd = format!("s {} {} {}", state, clock, voltage);
        self.modify_pstate(clock, voltage, &cmd, "SCLK")
    }

    pub fn modify_pstate_memory(&self, state: u32, clock: u32, voltage: u32) -> Result<(), OverclockError> {
        let cmd = format!("m {} {} {}", state, clock, voltage);
        self.modify_pstate(clock, voltage, &cmd, "MCLK")
    }

    const PSTATE_TABLE_FILE: &'static str = "pp_od_clk_voltage";

    pub fn commit_pstates(&self) {
        let path: PathBuf = self.sysfs_dir.join(Self::PSTATE_TABLE_FILE);
        Self::write(path, "c");
    }

    pub fn reset_pstates(&self) {
        let path: PathBuf = self.sysfs_dir.join(Self::PSTATE_TABLE_FILE);
        Self::write(path, "r");
    }

    fn modify_pstate(&self, clock: u32, voltage: u32, cmd: &String, kind: &'static str) 
            -> Result<(), OverclockError> {

        let path: PathBuf = self.sysfs_dir.join(Self::PSTATE_TABLE_FILE);

        if !path.is_file() {
            return Err(OverclockError::Disabled);
        }

        match Self::try_write(&path, &cmd) {
            Ok(_) => Ok(()),
            Err(err) => {
                match err.kind() {
                    std::io::ErrorKind::InvalidData => {
                        let data: String = Self::read_string_from_file(&path);
                        let clock_range = Self::parse_acceptable_range(&data, kind);
                        let voltage_range = Self::parse_acceptable_range(&data, "VDDC");

                        if !clock_range.contains(&clock) {
                            Err(OverclockError::ClockNotInRange(clock_range))
                        } else if !voltage_range.contains(&voltage) {
                            Err(OverclockError::VoltageNotInRange(voltage_range))
                        } else {
                            panic!("I don't know what I've done wrong! :(");
                        }
                    }
                    other => Err(OverclockError::Unknown(other))
                }
            }
        }
    }

    /**
        Example data:  
        OD_SCLK:
        0:        300MHz        750mV
        1:        588MHz        765mV
        2:        952MHz        931mV
        3:       1041MHz       1006mV
        4:       1106MHz       1068mV
        5:       1168MHz       1131mV
        6:       1209MHz       1150mV
        7:       1244MHz       1150mV
        OD_MCLK:
        0:        300MHz        750mV
        1:       1000MHz        800mV
        2:       1500MHz        900mV
        OD_RANGE:
        SCLK:     300MHz       2000MHz
        MCLK:     300MHz       2250MHz
        VDDC:     750mV        1150mV


        Returns the range specified for given OD_RANGE

        Example usage:
            parse_acceptable_range(*example data*, "MCLK")
        should return:
            RangeInclusive(300, 2250)
    **/
    fn parse_acceptable_range(data: &String, entry: &'static str) -> RangeInclusive::<u32> {
        let line = data
            .split("\n")
            .skip_while(|line| line.trim() != "OD_RANGE:")
            .filter(|line| line.contains(entry))
            .next().expect("No current memory pstate?!")
            .trim()
            .split(":")
            .skip(1)
            .next().expect("Range is not prefixed?!")
            .trim()
            .replace("MHz", "");

        let mut range_split = line.split_whitespace();

        let lower: &str = range_split.next().expect("Bad range lower bound");
        let upper: &str = range_split.next().expect("Bad range upper bound");

        let lower_parsed = lower.parse::<u32>().expect("Lower bound is not an integer");
        let upper_parsed = upper.parse::<u32>().expect("Upper bound is not an integer");

        RangeInclusive::new(lower_parsed, upper_parsed)
    }

    /**
        Example data:  
        0: 300Mhz  
        1: 1000Mhz *  
        2: 1500Mhz  

        Returns the index (line prefix) of current (* suffix) state
    **/
    fn parse_current_pstate(data: String) -> u32 {
        let current_state: &str = data
            .split("\n")
            .filter(|line| line.contains('*'))
            .next().expect("No current memory pstate?!")
            .trim()
            .split(":")
            .next().expect("State is not numbered?!");
            
        current_state.parse::<u32>().expect("State index is not a number")
    }

    const FORCE_PERFORMANCE_LEVEL_FILE: &'static str = "power_dpm_force_performance_level";

    pub fn force_performance_level(&self) -> PerformanceLevel {
        let path: PathBuf = self.sysfs_dir.join(Self::FORCE_PERFORMANCE_LEVEL_FILE);
        let data = Self::read_string_from_file(&path).trim().to_string();

        Self::PERFORMANCE_LEVEL_TO_STRING.iter()
            .filter(|(_, name)| data.eq_ignore_ascii_case(name))
            .next().expect("Invalid performance level").0.clone()
    }

    pub fn set_force_performance_level(&self, level: PerformanceLevel) {
        let path: PathBuf = self.sysfs_dir.join(Self::FORCE_PERFORMANCE_LEVEL_FILE);

        let value: &'static str = Self::PERFORMANCE_LEVEL_TO_STRING.iter()
            .filter(|(i_level, _)| i_level.eq(&level))
            .next().expect("Invalid performance level").1;

        Self::write(path, value);
    }

    const PCIE_LEVEL_FILE: &'static str = "pp_dpm_pcie";
    // TODO: Read real available levels, maybe split it into bandwidth and width

    pub fn pcie_level(&self) -> PcieLevel {
        let mode: u32 = Self::parse_string_from_file(&self.sysfs_dir.join(Self::PCIE_LEVEL_FILE));

        match mode {
            0 => PcieLevel::Gen1,
            1 => PcieLevel::Gen3,
            _ => panic!("Unknown PCIE mode")
        }
    }

    pub fn set_pcie_level(&self, mode: PcieLevel) {
        let value = match mode {
            PcieLevel::Gen1 => 0,
            PcieLevel::Gen3 => 1
        };
        let path = self.sysfs_dir.join(Self::PCIE_LEVEL_FILE);

        Self::write(path, &value.to_string());
    }


    pub fn has_sensor(&self, sensor: TemperatureSensor) -> bool {
        let index = self.get_sensor_index(sensor);

        let file = format!("hwmon/hwmon0/temp{}_input", index);
        let path = Path::new(&file);

        path.is_file()
    }

    pub fn read_sensor(&self, sensor: TemperatureSensor) -> std::option::Option<f32> {
        let path = self.get_sensor_path(sensor);

        if !path.is_file() {
            None
        } else {
            let value: f32 = Self::parse_string_from_file(&path);
            Some(value / 1000f32)
        }
    }

    const PERFORMANCE_LEVEL_TO_STRING: [(PerformanceLevel, &'static str); 8] = [
            (PerformanceLevel::Auto, "auto"),
            (PerformanceLevel::Low, "low"),
            (PerformanceLevel::High, "high"),
            (PerformanceLevel::Manual, "manual"),
            (PerformanceLevel::ProfileMinMclk, "profile_min_mclk"),
            (PerformanceLevel::ProfileMinSclk, "profile_min_sclk"),
            (PerformanceLevel::ProfilePeak, "profile_peak"),
            (PerformanceLevel::ProfileStandard, "profile_standard")
    ];

    fn try_write<P: AsRef<Path>>(path: P, value: &'a str) -> Result<(), std::io::Error> {

        let value_with_newline = format!("{}\n", value);

        match std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .create_new(false)
            .open(path.as_ref())
        {
            Ok(mut file) => {
                match file.write_all(value_with_newline.as_bytes()) {
                    Ok(_) => file.sync_all(),
                    Err(err) => Err(err)
                }
            }
            Err(err) => Err(err)
        }
    }

    const DEBUG: bool = false;

    fn write<P: AsRef<Path>>(path: P, value: &'a str) {

        if Self::DEBUG {
            let value_with_newline = format!("{}\n", value);
            let path_str = path.as_ref().to_str().unwrap();

            println!("Writing: {} -> {}", value_with_newline, path_str);
        }
        Self::try_write(path, value).expect("Failed to write file");
    }

    fn parse_string_from_file<T: std::str::FromStr, P: AsRef<Path>>(path: &P) -> T {
        let data: String = Self::read_string_from_file(path);

        match data.trim().parse::<T>() {
            Ok(parsed) => parsed,
            Err(_) => panic!("Could not parse {}", data)
        }
    }

    fn read_string_from_file<P: AsRef<Path>>(path: &P) -> String {
        let mut file = File::open(path).expect("Could not open file");
        let mut data = String::new();
        file.read_to_string(&mut data).expect("Could not read from file");

        data
    }

    fn get_sensor_path(&self, sensor: TemperatureSensor) -> PathBuf {
        let index = self.get_sensor_index(sensor);

        let file = format!("hwmon/hwmon0/temp{}_input", index);
        self.sysfs_dir.join(file)
    }

    fn get_sensor_index(&self, sensor: TemperatureSensor) -> u32 {
        match sensor {
            TemperatureSensor::Edge => 1,
            TemperatureSensor::Junction => 2,
            TemperatureSensor::Memory => 3
        }
    }

}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GpuCustomState {
    Idle,
    CoolOff,
    Performance
}

pub struct GpuStateMachine {
    state: GpuCustomState,
    usage_buffer: CircularBuffer::<f64>,
    temperature_buffer: CircularBuffer::<f32>
}

impl GpuStateMachine {

    pub fn state(&self) -> GpuCustomState {
        self.state
    }

    pub fn new(buffer_scale: usize) -> Self {
        GpuStateMachine {
            state: GpuCustomState::Idle,
            usage_buffer: CircularBuffer::new(30 * buffer_scale),
            temperature_buffer: CircularBuffer::new(10 * buffer_scale)
        }
    }

    pub fn update(&mut self, gpu: &PolarisGpu<'_>) {
        self.usage_buffer.add(gpu.usage().0);
        self.temperature_buffer.add(gpu.temperature());
    }

    pub fn step(&mut self, gpu: &PolarisGpu<'_>){
        let current_temperature = *self.temperature_buffer.last();
        let weighted_avg_usage = index_weighted_average(self.usage_buffer.iter());
        let weighted_avg_temperature = index_weighted_average(self.temperature_buffer.iter());
        let performance_treshold = 75f64;

        println!(" * {}C, weighted usage: {:.2}%, weighted temperature: {:.2}C",
            current_temperature, weighted_avg_usage, weighted_avg_temperature);

        let new_state = if weighted_avg_usage >= performance_treshold {
            GpuCustomState::Performance
        } else {
            match self.state {
                GpuCustomState::Idle => {
                    if weighted_avg_usage > 50f64 {
                        GpuCustomState::Performance
                    } else if current_temperature >= 55f32 {
                        GpuCustomState::CoolOff
                    } else {
                        self.state
                    }
                },
                GpuCustomState::CoolOff => {
                    if weighted_avg_temperature <= 43f32 {
                        GpuCustomState::Idle
                    } else {
                        self.state
                    }
                },
                GpuCustomState::Performance => {
                    if self.usage_buffer.iter().any(|usage| *usage >= performance_treshold) && weighted_avg_usage >= 5f64 {
                        self.state
                    } else {
                        GpuCustomState::Idle
                    }
                }
            }
        };

        if new_state != self.state {
            self.state = new_state;
            self.apply(gpu);
        }
    }

    fn apply(&self, gpu: &PolarisGpu<'_>) {
        println!("> Applying state {:?}", self.state);

        match self.state {
            GpuCustomState::Idle => {
                gpu.set_force_performance_level(PerformanceLevel::Manual);

                gpu.set_fan_mode(FanMode::Manual);
                gpu.set_fan_speed(ClampedPercentage::new(0));
                gpu.set_pcie_level(PcieLevel::Gen1);

                gpu.set_pstate_core(0);
                gpu.set_pstate_memory(0);
                gpu.set_power_limit(40f32);
            },
            GpuCustomState::Performance => {
                gpu.set_force_performance_level(PerformanceLevel::Manual);

                gpu.set_fan_mode(FanMode::Manual);
                gpu.set_fan_speed(ClampedPercentage::new(45));
                gpu.set_pcie_level(PcieLevel::Gen3);

                gpu.set_pstate_core(7);
                gpu.set_pstate_memory(2);
                gpu.set_power_limit(135f32);
            },
            GpuCustomState::CoolOff => {
                gpu.set_fan_mode(FanMode::Manual);
                gpu.set_fan_speed(ClampedPercentage::new(35));
                gpu.set_pcie_level(PcieLevel::Gen1);
            }
        }
    }
}

mod tests {

    #[test]
    fn fdfdd() {
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

        assert_eq!(PolarisGpu::parse_acceptable_range(&data.to_owned(), "SCLK"), RangeInclusive::new(300, 2000));
    }
}


fn main() {
    let rx570 = PolarisGpu {
        name: "RX 570",
        sysfs_dir: Path::new("/sys/class/drm/card0/device/")
    };

    let term = Arc::new(AtomicBool::new(false));

    signal_hook::flag::register(signal_hook::SIGTERM, Arc::clone(&term)).expect("Failed to register hook for SIGTERM");
    signal_hook::flag::register(signal_hook::SIGINT, Arc::clone(&term)).expect("Failed to register hook for SIGINT");

    let update_interval = time::Duration::from_secs_f32(1f32);
    let gathers_per_update = 2;
    let gathers_per_reapply = 10;

    let sleep_time = update_interval.div(gathers_per_update.try_into().unwrap());

    let mut gathers = 0;

    rx570.modify_pstate_core(7, 1275, 1000).expect("Core overlock failed");
    rx570.modify_pstate_memory(2, 1900, 900).expect("Memory overclock failed");
    rx570.commit_pstates();

    let mut state_machine = GpuStateMachine::new(gathers_per_update);
    state_machine.apply(&rx570);

    while !term.load(Ordering::Relaxed) {

        state_machine.update(&rx570);

        if gathers % gathers_per_update == 0 {

            println!("{} temperature: {}C, fan: {}, state: {:?}", rx570.name,
                rx570.temperature(), rx570.fan_speed(), state_machine.state());

            state_machine.step(&rx570);
        }

        if gathers % gathers_per_reapply == 0 {
            state_machine.apply(&rx570);
        }


        thread::sleep(sleep_time);
        gathers += 1;
    }

    rx570.set_force_performance_level(PerformanceLevel::Auto);
    rx570.reset_pstates();
    println!("Qutting...");
}
