//! Panorama stitching engine.

pub mod align;
pub mod features;
pub mod geometry;
pub mod photometric;
pub mod render;
pub mod seams;

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub enum StitchError {
    NotEnoughImages,
    NoOverlap,
    EmptyCanvas,
    TooLarge,
    Cancelled,
    Io(String),
}

impl std::fmt::Display for StitchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use crate::i18n::{tr, Key};
        let s = match self {
            StitchError::NotEnoughImages => tr(Key::ErrNotEnough).to_string(),
            StitchError::NoOverlap => tr(Key::ErrNoOverlap).to_string(),
            StitchError::EmptyCanvas => tr(Key::ErrEmptyCanvas).to_string(),
            StitchError::TooLarge => tr(Key::ErrTooLarge).to_string(),
            StitchError::Cancelled => tr(Key::Cancelled).to_string(),
            StitchError::Io(e) => e.clone(),
        };
        f.write_str(&s)
    }
}

impl From<anyhow::Error> for StitchError {
    fn from(e: anyhow::Error) -> Self {
        StitchError::Io(format!("{:#}", e))
    }
}

/// Shared cancellation flag.
#[derive(Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
    pub fn check(&self) -> Result<(), StitchError> {
        if self.is_cancelled() {
            Err(StitchError::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// Shared progress state (stage text + fraction).
#[derive(Clone, Default)]
pub struct Progress {
    frac: Arc<AtomicU32>,
    stage: Arc<Mutex<String>>,
}

impl Progress {
    pub fn set_fraction(&self, f: f32) {
        self.frac.store(f.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }
    pub fn fraction(&self) -> f32 {
        f32::from_bits(self.frac.load(Ordering::Relaxed))
    }
    pub fn set_stage(&self, s: impl Into<String>) {
        if let Ok(mut g) = self.stage.lock() {
            *g = s.into();
        }
        self.set_fraction(0.0);
    }
    pub fn stage(&self) -> String {
        self.stage.lock().map(|g| g.clone()).unwrap_or_default()
    }
    /// Progress view mapping [0,1] of a sub-task into [a,b] of this progress.
    pub fn sub(&self, a: f32, b: f32) -> SubProgress {
        SubProgress { p: self.clone(), a, b }
    }
}

pub struct SubProgress {
    p: Progress,
    a: f32,
    b: f32,
}

impl SubProgress {
    pub fn set(&self, f: f32) {
        self.p.set_fraction(self.a + (self.b - self.a) * f.clamp(0.0, 1.0));
    }
}
