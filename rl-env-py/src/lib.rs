//! PyO3 numpy binding over [`rl_env::batch::BatchEnv`]: exposes `reset`/`step`
//! as numpy arrays so the Python trainer (a `pufferlib.PufferEnv` subclass,
//! Task 7) can drive the batched multi-agent env. One `PyBatchEnv` == one
//! Rust engine hosting `num_duels` duels == `2*num_duels` agents. Parallel
//! engines must be separate PROCESSES (rs-pathfinder process-global state);
//! the Python side pins `num_envs == num_workers` accordingly (Plan B.2).

use numpy::{IntoPyArray, PyArray1, PyArray2, PyReadonlyArray2};
use pyo3::prelude::*;
use rl_env::batch::{BatchConfig, BatchEnv as CoreBatchEnv};

#[pyclass]
struct BatchEnv {
    inner: CoreBatchEnv,
}

#[pymethods]
impl BatchEnv {
    #[new]
    fn new(
        scenario_path: String,
        num_duels: usize,
        base_seed: u64,
        spot_stride: i32,
        reward_w: f32,
    ) -> Self {
        BatchEnv {
            inner: CoreBatchEnv::new(BatchConfig {
                scenario_path,
                num_duels,
                base_seed,
                spot_stride,
                reward_w,
            }),
        }
    }

    #[getter]
    fn num_agents(&self) -> usize {
        self.inner.num_agents()
    }
    #[getter]
    fn obs_stride(&self) -> usize {
        CoreBatchEnv::OBS_STRIDE
    }
    #[getter]
    fn act_stride(&self) -> usize {
        CoreBatchEnv::ACT_STRIDE
    }

    /// Returns the current observation buffer as `(num_agents, OBS_STRIDE)`.
    fn reset<'py>(&mut self, py: Python<'py>) -> Bound<'py, PyArray2<f32>> {
        let n = self.inner.num_agents();
        let mut obs = vec![0.0f32; n * CoreBatchEnv::OBS_STRIDE];
        self.inner.write_obs(&mut obs);
        obs.into_pyarray(py)
            .reshape([n, CoreBatchEnv::OBS_STRIDE])
            .unwrap()
    }

    /// Applies `actions` `(num_agents, ACT_STRIDE)` i32, advances one tick,
    /// and returns `(obs (N,OBS_STRIDE) f32, rewards (N,) f32, dones (N,) f32)`.
    fn step<'py>(
        &mut self,
        py: Python<'py>,
        actions: PyReadonlyArray2<i32>,
    ) -> (
        Bound<'py, PyArray2<f32>>,
        Bound<'py, PyArray1<f32>>,
        Bound<'py, PyArray1<f32>>,
    ) {
        let n = self.inner.num_agents();
        let a = actions.as_slice().expect("actions must be C-contiguous");
        let mut obs = vec![0.0f32; n * CoreBatchEnv::OBS_STRIDE];
        let mut rew = vec![0.0f32; n];
        let mut done = vec![0.0f32; n];
        self.inner.step(a, &mut obs, &mut rew, &mut done);
        (
            obs.into_pyarray(py)
                .reshape([n, CoreBatchEnv::OBS_STRIDE])
                .unwrap(),
            rew.into_pyarray(py),
            done.into_pyarray(py),
        )
    }
}

#[pymodule]
fn rs_pk_env(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<BatchEnv>()?;
    Ok(())
}
