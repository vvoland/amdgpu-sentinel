use crate::clamped_percentage::ClampedPercentage;
use crate::sysfs;
use crate::polaris_gpu_fan;

use std::path::Path;
use std::fmt::Display;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use polaris_gpu_fan::PolarisGpuFan;

pub struct PolarisGpu<'a> {
    pub name: &'a str,
    sysfs_dir: PathBuf,
    fan: PolarisGpuFan
}

pub enum TemperatureSensor {
    Edge,
    Junction,
    Memory
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

impl<'a> PolarisGpu<'a> {
    pub fn new<P: AsRef<Path>>(name: &'a str, sysfs_dir: P) -> Self {
        PolarisGpu {
            name,
            sysfs_dir: sysfs_dir.as_ref().to_path_buf(),
            fan: PolarisGpuFan::new(sysfs_dir.as_ref().join("hwmon/hwmon0"), 1)
        }
    }

    pub fn usage(&self) -> ClampedPercentage {
        let percent: u32 = sysfs::parse_string_from_file(&self.sysfs_dir.join("gpu_busy_percent"));
        ClampedPercentage::new(percent)
    }
    
    pub fn fan(&self) -> &PolarisGpuFan {
        &self.fan
    }

    pub fn temperature(&self) -> f32 {
        self.read_sensor(TemperatureSensor::Edge).expect("GPU has no temperature sensor!")
    }

    pub fn power_limit(&self) -> f32 {
        let wattage: f32 = sysfs::parse_string_from_file(&self.sysfs_dir.join("hwmon/hwmon0/power1_cap"));
        wattage / Self::WATTAGE_DIVISOR
    }

    const WATTAGE_DIVISOR: f32 = 1000000f32;
    fn to_real_wattage(value: f32) -> u32 { (value * Self::WATTAGE_DIVISOR) as u32 }

    pub fn power_limit_range(&self) -> RangeInclusive<f32> {
        let min: f32 = sysfs::parse_string_from_file(&self.sysfs_dir.join("hwmon/hwmon0/power1_cap_min"));
        let max: f32 = sysfs::parse_string_from_file(&self.sysfs_dir.join("hwmon/hwmon0/power1_cap_max"));

        let divisor = Self::WATTAGE_DIVISOR;

        RangeInclusive::new(min / divisor, max / divisor)
    }

    pub fn set_power_limit(&self, wattage: f32) -> () {
        let path: PathBuf = self.sysfs_dir.join("hwmon/hwmon0/power1_cap");
        let range: RangeInclusive<f32> = self.power_limit_range();

        if range.contains(&wattage) {
            let real_value: u32 = Self::to_real_wattage(wattage);
            sysfs::write(path, &real_value.to_string());
        } else {
            panic!("Wattage must be in range [{}, {}]", range.start(), range.end());
        }
    }

    const PSTATE_MEMORY_FILE: &'static str = "pp_dpm_mclk";
    const PSTATE_CORE_FILE: &'static str = "pp_dpm_sclk";

    pub fn pstate_memory(&self) -> u32 {
        let path: PathBuf = self.sysfs_dir.join(Self::PSTATE_MEMORY_FILE);
        let data: String = sysfs::read_string_from_file(&path);

        Self::parse_current_pstate(data)
    }

    pub fn set_pstate_memory(&self, state: u32) {
        let path: PathBuf = self.sysfs_dir.join(Self::PSTATE_MEMORY_FILE);
        sysfs::write(path, &state.to_string());
    }
 
    pub fn pstate_core(&self) -> u32 {
        let path: PathBuf = self.sysfs_dir.join(Self::PSTATE_CORE_FILE);
        let data: String = sysfs::read_string_from_file(&path);

        Self::parse_current_pstate(data)
    }

    pub fn set_pstate_core(&self, state: u32) {
        let path: PathBuf = self.sysfs_dir.join(Self::PSTATE_CORE_FILE);
        sysfs::write(path, &state.to_string());
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
        sysfs::write(path, "c");
    }

    pub fn reset_pstates(&self) {
        let path: PathBuf = self.sysfs_dir.join(Self::PSTATE_TABLE_FILE);
        sysfs::write(path, "r");
    }

    fn modify_pstate(&self, clock: u32, voltage: u32, cmd: &String, kind: &'static str) 
            -> Result<(), OverclockError> {

        let path: PathBuf = self.sysfs_dir.join(Self::PSTATE_TABLE_FILE);

        if !path.is_file() {
            return Err(OverclockError::Disabled);
        }

        match sysfs::try_write(&path, &cmd) {
            Ok(_) => Ok(()),
            Err(err) => {
                match err.kind() {
                    std::io::ErrorKind::InvalidData => {
                        let data: String = sysfs::read_string_from_file(&path);
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
            .replace("mV", "")
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
        let data = sysfs::read_string_from_file(&path).trim().to_string();

        Self::PERFORMANCE_LEVEL_TO_STRING.iter()
            .filter(|(_, name)| data.eq_ignore_ascii_case(name))
            .next().expect("Invalid performance level").0.clone()
    }

    pub fn set_force_performance_level(&self, level: PerformanceLevel) {
        let path: PathBuf = self.sysfs_dir.join(Self::FORCE_PERFORMANCE_LEVEL_FILE);

        let value: &'static str = Self::PERFORMANCE_LEVEL_TO_STRING.iter()
            .filter(|(i_level, _)| i_level.eq(&level))
            .next().expect("Invalid performance level").1;

        sysfs::write(path, value);
    }

    const PCIE_LEVEL_FILE: &'static str = "pp_dpm_pcie";
    // TODO: Read real available levels, maybe split it into bandwidth and width

    pub fn pcie_level(&self) -> PcieLevel {
        let mode: u32 = sysfs::parse_string_from_file(&self.sysfs_dir.join(Self::PCIE_LEVEL_FILE));

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

        sysfs::write(path, &value.to_string());
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
            let value: f32 = sysfs::parse_string_from_file(&path);
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

mod tests {

    #[test]
    fn parses_acceptable_range_from_pstate_table() {
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
        assert_eq!(PolarisGpu::parse_acceptable_range(&data.to_owned(), "MCLK"), RangeInclusive::new(300, 2250));
        assert_eq!(PolarisGpu::parse_acceptable_range(&data.to_owned(), "VDDC"), RangeInclusive::new(750, 1150));
    }
}

