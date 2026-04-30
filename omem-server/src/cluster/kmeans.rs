pub struct KMeansResult {
    pub labels: Vec<usize>,
    pub centroids: Vec<Vec<f32>>,
    pub k: usize,
}

pub fn kmeans(data: &[Vec<f32>], max_k: usize, max_iterations: usize) -> KMeansResult {
    let n = data.len();
    if n <= 1 {
        return KMeansResult {
            labels: vec![0; n],
            centroids: if n == 1 { vec![normalize(&data[0])] } else { Vec::new() },
            k: if n == 1 { 1 } else { 0 },
        };
    }

    // 内存保护：估算K-Means内存占用
    // 每条向量: 1024 dims × 4 bytes = 4KB
    // K-Means额外 ≈ 3x (points copy + centroids + labels)
    let estimated_mb = (n as u64 * 1024 * 4 * 3) / (1024 * 1024);
    if estimated_mb > 500 {
        tracing::warn!(
            n,
            estimated_mb,
            "K-Means would use >500MB, capping at 500 memories"
        );
        // 用前500条（它们按向量ID排序，基本是时间序的，够用了）
        let capped_data: Vec<Vec<f32>> = data.iter().take(500).cloned().collect();
        return kmeans(&capped_data, max_k, max_iterations);
    }

    let points: Vec<Vec<f32>> = data.iter().map(|v| normalize(v)).collect();
    let k = (n as f64 / 2.0).sqrt().ceil() as usize;
    let k = k.min(max_k).max(1).min(n);

    let mut centroids = kmeans_plus_plus(&points, k);
    let mut labels: Vec<usize> = vec![0; n];
    let mut changed = true;
    let mut iter = 0;

    while changed && iter < max_iterations {
        changed = false;
        iter += 1;

        for (i, point) in points.iter().enumerate() {
            let mut best_dist = f32::MAX;
            let mut best_j = 0;
            for (j, centroid) in centroids.iter().enumerate() {
                let d = cosine_distance(point, centroid);
                if d < best_dist {
                    best_dist = d;
                    best_j = j;
                }
            }
            if labels[i] != best_j {
                labels[i] = best_j;
                changed = true;
            }
        }

        let mut counts = vec![0usize; k];
        for &l in &labels {
            counts[l] += 1;
        }

        for j in 0..k {
            if counts[j] == 0 {
                let largest = counts
                    .iter()
                    .enumerate()
                    .max_by_key(|&(_, c)| c)
                    .map(|(idx, _)| idx)
                    .unwrap_or(0);

                let mut farthest_i = 0;
                let mut farthest_dist = -1.0f32;
                for (i, &label) in labels.iter().enumerate() {
                    if label == largest {
                        let d = cosine_distance(&points[i], &centroids[largest]);
                        if d > farthest_dist {
                            farthest_dist = d;
                            farthest_i = i;
                        }
                    }
                }

                labels[farthest_i] = j;
                counts[largest] -= 1;
                counts[j] += 1;
            }
        }

        let dim = points[0].len();
        for j in 0..k {
            let mut new_centroid = vec![0.0f32; dim];
            let mut count = 0usize;
            for (i, point) in points.iter().enumerate() {
                if labels[i] == j {
                    for d in 0..dim {
                        new_centroid[d] += point[d];
                    }
                    count += 1;
                }
            }
            if count > 0 {
                for d in 0..dim {
                    new_centroid[d] /= count as f32;
                }
                centroids[j] = normalize(&new_centroid);
            }
        }
    }

    KMeansResult {
        labels,
        centroids,
        k,
    }
}

fn l2_norm(v: &[f32]) -> f32 {
    v.iter().map(|x| x * x).sum::<f32>().sqrt()
}

fn normalize(v: &[f32]) -> Vec<f32> {
    let norm = l2_norm(v);
    if norm == 0.0 {
        return v.to_vec();
    }
    v.iter().map(|x| x / norm).collect()
}

fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
    let dot = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum::<f32>();
    (1.0 - dot).max(0.0).min(2.0)
}

fn kmeans_plus_plus(points: &[Vec<f32>], k: usize) -> Vec<Vec<f32>> {
    let n = points.len();
    let mut rng = fast_prng(n as u64);
    let mut centroids: Vec<Vec<f32>> = Vec::with_capacity(k);
    let mut chosen = vec![false; n];

    let first = rng.next_usize(n);
    centroids.push(points[first].clone());
    chosen[first] = true;

    for _ in 1..k {
        let mut max_dist = -1.0f32;
        let mut best_idx = 0;
        for (i, point) in points.iter().enumerate() {
            if chosen[i] {
                continue;
            }
            let dist = cosine_distance(point, &centroids[centroids.len() - 1]);
            if dist > max_dist {
                max_dist = dist;
                best_idx = i;
            }
        }
        centroids.push(points[best_idx].clone());
        chosen[best_idx] = true;
    }

    centroids
}

struct FastPrng {
    state: u64,
}

impl FastPrng {
    fn new(seed: u64) -> Self {
        Self { state: seed.wrapping_add(0x9E3779B97F4A7C15) }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    fn next_usize(&mut self, max: usize) -> usize {
        if max == 0 {
            return 0;
        }
        (self.next_u64() as usize) % max
    }
}

fn fast_prng(seed: u64) -> FastPrng {
    FastPrng::new(seed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kmeans_basic() {
        let data = vec![
            vec![1.0, 0.0, 0.0],
            vec![0.9, 0.1, 0.0],
            vec![0.0, 1.0, 0.0],
            vec![0.0, 0.9, 0.1],
        ];
        let result = kmeans(&data, 10, 100);
        assert_eq!(result.k, 2);
        assert_eq!(result.labels.len(), 4);
        assert_eq!(result.labels[0], result.labels[1]);
        assert_eq!(result.labels[2], result.labels[3]);
        assert_ne!(result.labels[0], result.labels[2]);
    }

    #[test]
    fn test_kmeans_empty_input() {
        let data: Vec<Vec<f32>> = vec![];
        let result = kmeans(&data, 10, 100);
        assert_eq!(result.k, 0);
        assert!(result.labels.is_empty());
    }

    #[test]
    fn test_kmeans_single_point() {
        let data = vec![vec![1.0, 2.0, 3.0]];
        let result = kmeans(&data, 10, 100);
        assert_eq!(result.k, 1);
        assert_eq!(result.labels, vec![0]);
    }
}
