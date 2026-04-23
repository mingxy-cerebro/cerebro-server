use std::fmt;

/// Per-stage diagnostics for the retrieval pipeline.
#[derive(Debug, Clone)]
pub struct StageTrace {
    pub name: String,
    pub input_count: usize,
    pub output_count: usize,
    pub dropped_ids: Vec<String>,
    pub score_range: Option<(f32, f32)>,
    pub duration_ms: u64,
}

/// Aggregated trace across all pipeline stages.
#[derive(Debug, Clone)]
pub struct RetrievalTrace {
    pub stages: Vec<StageTrace>,
    pub total_duration_ms: u64,
    pub final_count: usize,
}

impl RetrievalTrace {
    pub fn new() -> Self {
        Self {
            stages: Vec::new(),
            total_duration_ms: 0,
            final_count: 0,
        }
    }

    pub fn add_stage(&mut self, stage: StageTrace) {
        self.stages.push(stage);
    }

    pub fn finalize(&mut self, final_count: usize, total_ms: u64) {
        self.final_count = final_count;
        self.total_duration_ms = total_ms;
    }
}

impl Default for RetrievalTrace {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for RetrievalTrace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "╔══ Retrieval Trace ══════════════════════")?;
        for (i, stage) in self.stages.iter().enumerate() {
            writeln!(
                f,
                "║ Stage {}: {} ({} → {} in {}ms)",
                i + 1,
                stage.name,
                stage.input_count,
                stage.output_count,
                stage.duration_ms,
            )?;
            if let Some((min, max)) = stage.score_range {
                writeln!(f, "║   scores: {min:.4}..{max:.4}")?;
            }
            if !stage.dropped_ids.is_empty() {
                writeln!(f, "║   dropped: {} ids", stage.dropped_ids.len())?;
            }
        }
        writeln!(
            f,
            "╚══ {} results in {}ms ══════════════════",
            self.final_count, self.total_duration_ms,
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_display_format() {
        let mut trace = RetrievalTrace::new();
        trace.add_stage(StageTrace {
            name: "parallel_search".to_string(),
            input_count: 0,
            output_count: 12,
            dropped_ids: vec![],
            score_range: Some((0.35, 0.95)),
            duration_ms: 42,
        });
        trace.add_stage(StageTrace {
            name: "rrf_fusion".to_string(),
            input_count: 12,
            output_count: 8,
            dropped_ids: vec!["id-1".to_string()],
            score_range: Some((0.02, 0.85)),
            duration_ms: 1,
        });
        trace.finalize(8, 43);

        let output = trace.to_string();
        assert!(output.contains("Retrieval Trace"));
        assert!(output.contains("parallel_search"));
        assert!(output.contains("rrf_fusion"));
        assert!(output.contains("8 results in 43ms"));
        assert!(output.contains("dropped: 1 ids"));
    }

    #[test]
    fn trace_default() {
        let trace = RetrievalTrace::default();
        assert!(trace.stages.is_empty());
        assert_eq!(trace.total_duration_ms, 0);
        assert_eq!(trace.final_count, 0);
    }
}
