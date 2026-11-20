//! p50, p95 et pire d'une série de durées (au plus proche rang).

use std::time::Duration;

pub fn summary(times: &[Duration]) -> String {
    if times.is_empty() {
        return "—".into();
    }
    let mut sorted = times.to_vec();
    sorted.sort();
    let at = |q: f64| {
        let rank = ((q * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len());
        sorted[rank - 1]
    };
    format!(
        "p50 {} / p95 {} / pire {} (n={})",
        ms(at(0.5)),
        ms(at(0.95)),
        ms(*sorted.last().unwrap()),
        sorted.len()
    )
}

pub fn ms(d: Duration) -> String {
    let v = d.as_secs_f64() * 1000.0;
    if v >= 100.0 {
        format!("{v:.0} ms")
    } else if v >= 10.0 {
        format!("{v:.1} ms")
    } else {
        format!("{v:.2} ms")
    }
}

pub fn median(times: &[Duration]) -> Duration {
    let mut sorted = times.to_vec();
    sorted.sort();
    sorted[(sorted.len() - 1) / 2]
}
