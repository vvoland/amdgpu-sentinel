use crate::fan::FanMode;
use crate::generic_sysfs_fan::{GenericSysFsFan, build_sysfs_paths};

use std::path::PathBuf;
use std::path::Path;

pub struct Nct6797Fan {
    sysfs_pwm_file: PathBuf,
    sysfs_pwm_enable_file: PathBuf
}

impl<'a> GenericSysFsFan for Nct6797Fan {
    fn sysfs_pwm_file(&self) -> &PathBuf { &self.sysfs_pwm_file }

    fn sysfs_pwm_enable_file(&self) -> &PathBuf { &self.sysfs_pwm_enable_file }

    fn parse_mode(value: u8) -> FanMode {
        match value {
            1 => FanMode::Manual,
            5 => FanMode::Auto,
            _ => panic!("Unknown mode value")
        }
    }

    fn dump_mode(mode: FanMode) -> u8 {
        match mode {
            FanMode::Manual => 1,
            FanMode::Auto => 5
        }
    }
}

impl<'a> Nct6797Fan {
    pub fn new<P: AsRef<Path>>(sysfs_dir: P, index: u32) -> Self {
        match build_sysfs_paths(sysfs_dir, index) {
            Some((pwm_file, pwm_enable_file)) => Nct6797Fan {
                sysfs_pwm_file: pwm_file,
                sysfs_pwm_enable_file: pwm_enable_file
            },
            None => panic!("Invalid pwm fan")
        }
    }
}
