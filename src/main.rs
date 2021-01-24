use std::convert::TryInto;
use std::ops::Div;
use std::path::Path;
use std::{thread, time};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

extern crate signal_hook;
extern crate num;

mod fan;
use fan::*;
mod sysfs;
mod polaris_gpu;
use polaris_gpu::*;
mod clamped_percentage;
use clamped_percentage::*;
mod stats;
use stats::*;
mod circular_buffer;
use circular_buffer::CircularBuffer;
mod polaris_gpu_fan;
mod generic_sysfs_fan;
mod nct6797_fan;
mod polaris_gpu_table;
use polaris_gpu_table::{PolarisGpuTable, PolarisGpuState};
mod performance_level;
use performance_level::{PerformanceLevel, ControllablePerformanceLevel};
mod amdgpu_performance_level;
mod sysfs_device;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuCustomState {
    Idle,
    CoolOff,
    Performance
}

pub struct GpuStateMachine {
    state: GpuCustomState,
    usage_buffer: CircularBuffer::<f64>,
    temperature_buffer: CircularBuffer::<f32>,
    power_usage_buffer: CircularBuffer::<f32>,
    idle_table: PolarisGpuTable,
    performance_table: PolarisGpuTable,
    performance_curve: Curve
}

#[derive(Clone)]
pub struct CurvePoint {
    temperature: u32,
    fan_speed: ClampedPercentage
}

pub struct Curve {
    points: Vec<CurvePoint>
}

pub enum CurveInterpolation {
    Linear
}

impl Curve {
    pub fn new(points: Vec::<CurvePoint>) -> Self {
        if points.len() < 1 {
            panic!("Invalid curve without any point");
        }
        let mut points_vec = points.to_vec();
        points_vec.sort_by_key(|pt| pt.temperature);

        Curve { points: points_vec }
    }

    fn interpolate(value: f32, lower: &CurvePoint, upper: &CurvePoint, interpolation: CurveInterpolation) -> ClampedPercentage {
        let temp_delta: f32 = (upper.temperature - lower.temperature) as f32;
        let speed_delta: f32 = (upper.fan_speed.0 - lower.fan_speed.0) as f32;
        let diff: f32 = value - lower.temperature as f32;

        let value: f32 = match interpolation {
            CurveInterpolation::Linear => {
                (lower.fan_speed.0 as f32) + (diff / temp_delta) * speed_delta
            }
        };

        ClampedPercentage::new(value)
    }

    pub fn get(&self, temperature: f32, interpolation: CurveInterpolation) -> ClampedPercentage {
        for (idx, point) in self.points.iter().enumerate().rev() {
            if temperature as u32 >= point.temperature {
                // Get next point or the same if last
                let next_point = self.points.get(idx + 1).unwrap_or(point);
                return Self::interpolate(temperature, point, next_point, interpolation);
            }
        }

        self.points.get(0).expect("Curve must have at least one point").fan_speed
    }
}

impl GpuStateMachine {

    pub fn state(&self) -> GpuCustomState {
        self.state
    }

    pub fn new(buffer_scale: usize, idle_table: PolarisGpuTable, performance_table: PolarisGpuTable, curve: Curve) -> Self {
        GpuStateMachine {
            state: GpuCustomState::Idle,
            usage_buffer: CircularBuffer::new(20 * buffer_scale),
            temperature_buffer: CircularBuffer::new(10 * buffer_scale),
            power_usage_buffer: CircularBuffer::new(5 * buffer_scale),
            performance_curve: curve,
            idle_table,
            performance_table
        }
    }

    pub fn update(&mut self, gpu: &PolarisGpu<'_>) {
        self.usage_buffer.add(gpu.usage().0);
        self.temperature_buffer.add(gpu.temperature());
        self.power_usage_buffer.add(gpu.power_usage());
    }

    pub fn step(&mut self, gpu: &PolarisGpu<'_>){
        let current_temperature = *self.temperature_buffer.last();
        let weighted_avg_usage = index_weighted_average(self.usage_buffer.iter());
        let weighted_avg_temperature = index_weighted_average(self.temperature_buffer.iter());
        let weighted_avg_power_usage = index_weighted_average(self.power_usage_buffer.iter());
        let performance_treshold = 90f64;
        let power_treshold = 50f32;

        println!(" * {}C, weighted usage: {:.2}%, weighted temperature: {:.2}C",
            current_temperature, weighted_avg_usage, weighted_avg_temperature);

        let new_state = if weighted_avg_usage > 95f64 || (weighted_avg_usage > 0.5f64 && weighted_avg_power_usage > 40f32) {
            GpuCustomState::Performance
        } else {
            match self.state {
                GpuCustomState::Idle => {
                    if weighted_avg_usage > performance_treshold {
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
                    if weighted_avg_power_usage > power_treshold || weighted_avg_usage >= 10f64 {
                        self.state
                    } else {
                        GpuCustomState::Idle
                    }
                }
            }
        };

        if new_state != self.state {
            self.apply(gpu, new_state);
        }
        self.apply_dynamic(gpu, new_state, weighted_avg_temperature);
        self.state = new_state;
    }

    fn apply_dynamic(&self, gpu: &PolarisGpu<'_>, state: GpuCustomState, temperature: f32) {
        if state == GpuCustomState::Performance {
            gpu.fan().set_speed(self.performance_curve.get(temperature, CurveInterpolation::Linear));
        }
    }

    fn apply(&self, gpu: &PolarisGpu<'_>, state: GpuCustomState) {
        println!("> Applying state {:?}", self.state);

        match state {
            GpuCustomState::Idle => {

                if self.state != GpuCustomState::CoolOff {
                    gpu.set_pstates(&self.idle_table).expect("Failed to change gpu pstate table");
                }

                gpu.set_performance_level(PerformanceLevel::Manual);

                gpu.fan().set_mode(FanMode::Manual);
                gpu.fan().set_speed(ClampedPercentage::new(0));
                gpu.set_power_limit(30f32);
                gpu.set_power_profile_mode(2);
            },
            GpuCustomState::Performance => {
                gpu.set_pstates(&self.performance_table).expect("Failed to change gpu pstate table");

                gpu.set_performance_level(PerformanceLevel::Auto);

                gpu.fan().set_mode(FanMode::Manual);
                gpu.set_power_limit(150f32);
            },
            GpuCustomState::CoolOff => {
                gpu.fan().set_mode(FanMode::Manual);
                gpu.fan().set_speed(ClampedPercentage::new(35));
            }
        }
    }
}

fn create_idle_table<'a>(table: &'a PolarisGpuTable) -> PolarisGpuTable {
    let mut idle_table: PolarisGpuTable = table.clone();

    for part in [Part::Core, Part::Memory].iter() {
        let lowest_pstate = table.get_state(*part, 0).expect("Failed to get lowest state");
        for idx in 0..idle_table.states(*part).len() {
            idle_table.set_state(*part, idx, lowest_pstate).unwrap();
        }
    }
    idle_table
}

fn create_performance_table<'a>(table: &'a PolarisGpuTable,
    highest_core_state: &PolarisGpuState,
    highest_memory_state: &PolarisGpuState,
    fixed_memory: bool)
-> PolarisGpuTable {
    let mut perf_table = table.clone();

    let dynamic_parts = if fixed_memory {
        vec![Part::Core]
    } else {
        vec![Part::Core, Part::Memory]
    };

    for part in dynamic_parts.iter() {
        let highest_state = match part {
            Part::Core => highest_core_state,
            Part::Memory => highest_memory_state,
        };

        let states = perf_table.states(*part).clone();
        let count = states.len();

        for (idx, state) in states.iter().enumerate() {
            let mut new_state: PolarisGpuState = state.clone();
            if new_state.voltage > highest_state.voltage {
                new_state.voltage = highest_state.voltage;
            }
            if new_state.clock > highest_state.clock {
                new_state.clock = highest_state.clock;
            }

            perf_table.set_state(*part, idx, new_state).unwrap();
        }
        perf_table.set_state(*part, count - 1, *highest_state).unwrap();
    }

    if fixed_memory {
        for (idx, _) in table.states(Part::Memory).iter().enumerate() {
            perf_table.set_state(Part::Memory, idx, *highest_memory_state).unwrap();
        }
    }

    perf_table
}


fn main() {
    let rx570 = PolarisGpu::new("RX 570", Path::new("/sys/class/drm/card0/device/"));
    let term = Arc::new(AtomicBool::new(false));

    signal_hook::flag::register(signal_hook::SIGTERM, Arc::clone(&term)).expect("Failed to register hook for SIGTERM");
    signal_hook::flag::register(signal_hook::SIGINT, Arc::clone(&term)).expect("Failed to register hook for SIGINT");

    let update_interval = time::Duration::from_secs_f32(1f32);
    let gathers_per_update = 2;

    let sleep_time = update_interval.div(gathers_per_update.try_into().unwrap());

    let mut gathers = 0;

    let old_power_limit = rx570.power_limit();

    let curve = Curve::new(vec![
        CurvePoint { temperature: 50, fan_speed: ClampedPercentage::new(0f64) },
        CurvePoint { temperature: 55, fan_speed: ClampedPercentage::new(30f64) },
        CurvePoint { temperature: 65, fan_speed: ClampedPercentage::new(35f64) },
        CurvePoint { temperature: 75, fan_speed: ClampedPercentage::new(45f64) },
        //CurvePoint { temperature: 80, fan_speed: ClampedPercentage::new(70f64) }
    ]);

    let gpu_table: PolarisGpuTable = rx570.read_pstates().expect("Failed to read gpu pstates");
    let idle_table: PolarisGpuTable = create_idle_table(&gpu_table);
    let performance_table: PolarisGpuTable = create_performance_table(&gpu_table,
        &PolarisGpuState { clock: 1270, voltage: 1025 },
        &PolarisGpuState { clock: 1700, voltage: 900 },
        false);

    println!("Idle table\r\n{}\r\nPerformance\r\n{}", idle_table, performance_table);

    let mut state_machine = GpuStateMachine::new(gathers_per_update, idle_table, mining_table, curve);
    state_machine.apply(&rx570, GpuCustomState::Idle);

    while !term.load(Ordering::Relaxed) {

        state_machine.update(&rx570);

        if gathers % gathers_per_update == 0 {

            println!("{} temperature: {}C, fan: {}, state: {:?}", rx570.name,
                rx570.temperature(), rx570.fan().speed(), state_machine.state());

            state_machine.step(&rx570);
        }

        thread::sleep(sleep_time);
        gathers += 1;
    }

    rx570.fan().set_mode(FanMode::Auto);
    rx570.set_power_profile_mode(1);
    rx570.set_power_limit(old_power_limit);
    rx570.set_performance_level(PerformanceLevel::Auto);
    rx570.reset_pstates();
    println!("Qutting...");
}
