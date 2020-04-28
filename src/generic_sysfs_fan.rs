use crate::fan::FanMode;
use crate::clamped_percentage::ClampedPercentage;
use crate::fan::FanControl;
use crate::sysfs;

use std::path::PathBuf;
use std::path::Path;

pub trait GenericSysFsFan {
    fn sysfs_pwm_file(&self) -> &PathBuf;
    fn sysfs_pwm_enable_file(&self) -> &PathBuf;

    fn parse_mode(value: u8) -> FanMode;
    fn dump_mode(mode: FanMode) -> u8;
}

pub fn build_sysfs_paths<P: AsRef<Path>>(sysfs_dir: P, index: u32) -> Option<(PathBuf, PathBuf)> {
    let sysfs_dir_ref = sysfs_dir.as_ref();
    let base_file: PathBuf = sysfs_dir_ref.to_path_buf();

    if !base_file.is_dir() {
        return None;
    }

    let pwm_file: PathBuf = sysfs_dir_ref.join(format!("pwm{}", index));
    let pwm_enable_file: PathBuf = sysfs_dir_ref.join(format!("pwm{}_enable", index));

    for path in [&pwm_file, &pwm_enable_file].iter() {
        if !path.is_file() {
            return None;
        }
    }

    return Some((pwm_file, pwm_enable_file));
}

impl<T: GenericSysFsFan> FanControl for T {
    fn speed(&self) -> ClampedPercentage {
        let value: u8 = sysfs::parse_string_from_file(&self.sysfs_pwm_file());

        ClampedPercentage::new(value as f32 / 255f32 * 100f32)
    }

    fn set_speed(&self, speed: ClampedPercentage) {
        let value: u8 = (speed.0 * 255f64 / 100f64) as u8;

        sysfs::write(&self.sysfs_pwm_file(), &value.to_string());
    }

    fn mode(&self) -> FanMode { 
        let value: u8 = sysfs::parse_string_from_file(&self.sysfs_pwm_enable_file());

        T::parse_mode(value)
    }

    fn set_mode(&self, mode: FanMode) {
        let value = T::dump_mode(mode);

        sysfs::write(&self.sysfs_pwm_enable_file(), &value.to_string());
    }
}
