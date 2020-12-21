use crate::performance_level;
use crate::sysfs;
use crate::sysfs_device;

use std::path::PathBuf;
use performance_level::{ControllablePerformanceLevel, PerformanceLevel};
use sysfs_device::SysfsDevice;

pub trait AmdGpuSysfsPerformanceLevel {
    fn performance_level_file(&self) -> &'static str;
}

static PERFORMANCE_LEVEL_TO_STRING: [(PerformanceLevel, &'static str); 8] = [
        (PerformanceLevel::Auto, "auto"),
        (PerformanceLevel::Low, "low"),
        (PerformanceLevel::High, "high"),
        (PerformanceLevel::Manual, "manual"),
        (PerformanceLevel::ProfileMinMclk, "profile_min_mclk"),
        (PerformanceLevel::ProfileMinSclk, "profile_min_sclk"),
        (PerformanceLevel::ProfilePeak, "profile_peak"),
        (PerformanceLevel::ProfileStandard, "profile_standard")
];

impl<T: AmdGpuSysfsPerformanceLevel + SysfsDevice> ControllablePerformanceLevel for T {

    fn performance_level(&self) -> PerformanceLevel {
        let path: PathBuf = self.sysfs_dir().join(self.performance_level_file());
        let data = sysfs::read_string_from_file(&path).trim().to_string();

        PERFORMANCE_LEVEL_TO_STRING.iter()
            .filter(|(_, name)| data.eq_ignore_ascii_case(name))
            .next().expect("Invalid performance level").0.clone()
    }

    fn set_performance_level(&self, level: PerformanceLevel) {
        let path: PathBuf = self.sysfs_dir().join(self.performance_level_file());
        let value: &'static str = PERFORMANCE_LEVEL_TO_STRING.iter()
            .filter(|(i_level, _)| i_level.eq(&level))
            .next().expect("Invalid performance level").1;

        sysfs::write(path, value);
    }
}
