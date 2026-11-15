//! `generate_and_validate` (P6-T2): the generate-and-validate job of
//! `parble_sim::procgen_job`, sliced by a time budget for the generator Web Worker
//! (`web/ui/src/gen/`). Only this wrapper reads a clock; the job's result is the same however
//! the budget slices it.

use parble_core::ParbleFile;
use parble_procgen::{Families, GenParams};
use parble_sim::procgen_job::{GenJob, JobError, JobStatus, golden_lines, result_line};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::{JsError, wasm_bindgen};

/// The generator sliders as the worker sends them. Every field is optional and defaults to
/// [`GenParams::default`].
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ParamsWire {
    pub footprint: Option<u32>,
    pub height: Option<u32>,
    pub length: Option<u32>,
    pub special_per_mille: Option<u32>,
    pub difficulty: Option<u32>,
    pub families: Option<FamiliesWire>,
    pub marbles: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FamiliesWire {
    pub blk: bool,
    pub rail: bool,
    pub mech: bool,
}

impl ParamsWire {
    pub fn params(&self) -> GenParams {
        let d = GenParams::default();
        GenParams {
            footprint: self.footprint.unwrap_or(d.footprint),
            height: self.height.unwrap_or(d.height),
            length: self.length.unwrap_or(d.length),
            special_per_mille: self.special_per_mille.unwrap_or(d.special_per_mille),
            difficulty: self.difficulty.unwrap_or(d.difficulty),
            families: self.families.as_ref().map_or(d.families, |f| Families {
                blk: f.blk,
                rail: f.rail,
                mech: f.mech,
            }),
            marbles: self.marbles.unwrap_or(d.marbles),
        }
    }
}

/// One rejected candidate, for the worker's progress messages.
#[derive(Debug, Serialize)]
pub struct RejectedWire {
    pub attempt: u32,
    pub seed: u64,
    pub reason: String,
}

/// A job's state after a slice.
#[derive(Debug, Serialize)]
pub struct StatusWire {
    /// `running`, `accepted` or `failed`.
    pub status: &'static str,
    /// The candidate being tried (or accepted), 0-based.
    pub attempt: u32,
    /// Share of the current candidate's work done, 0 to 1.
    pub progress: f32,
    /// Work units done so far.
    pub units: u64,
    /// The accepted track's seed, once accepted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    pub rejected: Vec<RejectedWire>,
}

fn now_ms() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64() * 1000.0)
    }
}

/// A resumable generate-and-validate job. Every [`GenerateJob::resume`] does at least one work
/// unit, then more while its time budget lasts.
#[wasm_bindgen]
pub struct GenerateJob {
    job: GenJob,
    units: u64,
}

impl GenerateJob {
    /// A job from a JavaScript seed (an integer in `0..=MAX_SAFE_INTEGER`) and params JSON.
    pub fn create(seed: f64, params_json: &str) -> Result<Self, String> {
        if !(seed.is_finite() && seed >= 0.0 && seed.fract() == 0.0) {
            return Err(format!("seed {seed} is not a non-negative integer"));
        }
        let wire: ParamsWire =
            serde_json::from_str(params_json).map_err(|e| format!("params: {e}"))?;
        let job = GenJob::new(seed as u64, wire.params()).map_err(|e| match e {
            JobError::SeedOutOfRange(s) => format!("seed {s} is above the safe integer range"),
        })?;
        Ok(Self { job, units: 0 })
    }

    /// Run work units for `budget_ms` (at least one) and return the status.
    pub fn run_slice(&mut self, budget_ms: f64) -> StatusWire {
        let start = now_ms();
        loop {
            self.units += 1;
            if self.job.step() != JobStatus::Running || now_ms() - start >= budget_ms {
                break;
            }
        }
        self.status()
    }

    pub fn status(&self) -> StatusWire {
        StatusWire {
            status: match self.job.status() {
                JobStatus::Running => "running",
                JobStatus::Accepted => "accepted",
                JobStatus::Failed => "failed",
            },
            attempt: self.job.attempt(),
            progress: self.job.candidate_progress(),
            units: self.units,
            seed: self.job.accepted().map(|t| t.seed),
            rejected: self
                .job
                .rejected()
                .iter()
                .map(|a| RejectedWire {
                    attempt: a.attempt,
                    seed: a.seed,
                    reason: a.rejection.to_string(),
                })
                .collect(),
        }
    }

    /// The job's golden line (`parble_sim::procgen_job::result_line`).
    pub fn result_line(&self, base: u64) -> String {
        result_line(base, &self.job)
    }
}

#[wasm_bindgen]
impl GenerateJob {
    /// Run work units for `budget_ms` milliseconds (at least one) and return the status JSON.
    pub fn resume(&mut self, budget_ms: f64) -> String {
        serde_json::to_string(&self.run_slice(budget_ms)).expect("status serializes")
    }

    /// The status JSON without doing any work.
    #[wasm_bindgen(js_name = statusJson)]
    pub fn status_json(&self) -> String {
        serde_json::to_string(&self.status()).expect("status serializes")
    }

    /// The accepted track as a `.parble` file, once accepted.
    #[wasm_bindgen(js_name = fileJson)]
    pub fn file_json(&self, name: &str) -> Option<String> {
        self.job
            .accepted()
            .map(|t| ParbleFile::new(t.clone(), name).to_json())
    }

    /// The golden line of a finished job started at `base`.
    #[wasm_bindgen(js_name = resultLine)]
    pub fn result_line_js(&self, base: f64) -> String {
        self.result_line(base as u64)
    }
}

/// Start a generate-and-validate job for `seed` and the params JSON and run its first slice of
/// `budget_ms`. Call [`GenerateJob::resume`] until its status is no longer `running`.
#[wasm_bindgen]
pub fn generate_and_validate(
    seed: f64,
    params_json: &str,
    budget_ms: f64,
) -> Result<GenerateJob, JsError> {
    let mut job = GenerateJob::create(seed, params_json).map_err(|e| JsError::new(&e))?;
    job.run_slice(budget_ms);
    Ok(job)
}

/// The golden generate-and-validate jobs, each run to the end, and return the lines of
/// `crates/parble-sim/tests/golden/procgen_job.txt`.
#[wasm_bindgen]
pub fn run_procgen_job() -> String {
    golden_lines()
}
