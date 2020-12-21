use std::path::PathBuf;

pub trait SysfsDevice {
    fn sysfs_dir(&self) -> &PathBuf;
}
