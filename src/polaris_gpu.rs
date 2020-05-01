use crate::clamped_percentage::ClampedPercentage;
use crate::sysfs;
use crate::polaris_gpu_fan;
use crate::polaris_gpu_table;

use std::path::Path;
use std::fmt::Display;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use polaris_gpu_fan::PolarisGpuFan;
use polaris_gpu_table::PolarisGpuTable;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Part {
    Core,
    Memory
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
    Unknown(std::io::ErrorKind),
    RangesAreImmutable
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

    pub fn power_usage(&self) -> f32 {
        let wattage: f32 = sysfs::parse_string_from_file(&self.sysfs_dir.join("hwmon/hwmon0/power1_average"));
        wattage / Self::WATTAGE_DIVISOR
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

    pub fn read_pstates(&self) -> Option<PolarisGpuTable> {
         sysfs::try_read_string_from_file(&self.sysfs_dir.join(Self::PSTATE_TABLE_FILE))
             .map_or(None, |data| PolarisGpuTable::try_parse(&data))
    }

    const PSTATE_TABLE_FILE: &'static str = "pp_od_clk_voltage";

    fn table_to_commands(table: &PolarisGpuTable) -> Vec::<String> {
        let mut commands = Vec::new();
        for part in [Part::Core, Part::Memory].iter() {
            let states = table.states(*part);

            let prefix = match part {
                Part::Core => "s",
                Part::Memory => "m"
            };

            for (idx, state) in states.iter().enumerate() {
                commands.push(format!("{} {} {} {}", prefix, idx, state.clock, state.voltage));
            }
        }
        commands
    }

    pub fn set_pstates(&self, new_table: &PolarisGpuTable) -> Result<(), OverclockError> {
        match self.read_pstates() {
            Some(current_table) => {
                if current_table.voltage_range().eq(new_table.voltage_range()) &&
                    current_table.clock_range(Part::Core).eq(new_table.clock_range(Part::Core)) &&
                    current_table.clock_range(Part::Memory).eq(new_table.clock_range(Part::Memory))
                {
                    let current_table_cmds = Self::table_to_commands(&current_table);
                    let mut new_table_cmds = Self::table_to_commands(&new_table);
                    new_table_cmds.retain(|element| !current_table_cmds.contains(element));

                    let path = self.sysfs_dir.join(Self::PSTATE_TABLE_FILE);

                    let mut revert = false;
                    for cmd in new_table_cmds.iter() {
                        if sysfs::try_write(&path, &cmd).is_err() {
                            revert = true;
                            break;
                        }
                    };

                    if !revert {
                        if new_table_cmds.len() > 0 {
                            sysfs::write(path, "c");
                        }
                        Ok(())
                    } else {
                        self.reset_pstates();
                        Err(OverclockError::Disabled)
                    }
                } else {
                    Err(OverclockError::RangesAreImmutable)
                }
            },
            None => Err(OverclockError::Disabled)
        }
    }

    pub fn reset_pstates(&self) {
        let path: PathBuf = self.sysfs_dir.join(Self::PSTATE_TABLE_FILE);
        sysfs::write(path, "r");
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

