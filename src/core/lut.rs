use std::{fs, path::{Path, PathBuf}};

use sha2::{Digest, Sha256};

use super::{CoreError, CoreResult};

#[derive(Clone, Debug)]
pub struct Lut3d {
    pub size: usize,
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
    /// Red changes fastest, followed by green and blue.
    pub values: Vec<[f32; 3]>,
    pub source_path: PathBuf,
    pub sha256: String,
}

fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::InvalidProject(format!("无法读取 .cube：{}", message.into()))
}

pub fn load_cube(path: &Path) -> CoreResult<Lut3d> {
    if path.metadata()?.len() > 64 * 1024 * 1024 {
        return Err(invalid("文件超过64MB"));
    }
    let bytes = fs::read(path)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| invalid("需要UTF-8或ASCII文本"))?;
    let mut lut = parse_cube(text)?;
    lut.source_path = path.to_path_buf();
    lut.sha256 = format!("{:x}", Sha256::digest(&bytes));
    Ok(lut)
}

fn parse_cube(text: &str) -> CoreResult<Lut3d> {
    let mut size = 0;
    let mut values = Vec::new();
    let mut domain_min = [0.0; 3];
    let mut domain_max = [1.0; 3];
    let mut bounds = [false; 2];
    for line in text.trim_start_matches('\u{feff}').lines() {
        let tokens = line.split('#').next().unwrap_or("").split_whitespace().collect::<Vec<_>>();
        if tokens.is_empty() { continue; }
        let command = tokens[0].to_ascii_uppercase();
        let number = |token: &str| -> CoreResult<f32> {
            let n: f32 = token.parse().map_err(|_| invalid("非数值采样"))?;
            if n.is_finite() { Ok(n) } else { Err(invalid("采样必须为有限值")) }
        };
        match command.as_str() {
            "TITLE" => {},
            "LUT_3D_SIZE" if tokens.len() == 2 && size == 0 => {
                size = tokens[1].parse::<usize>().map_err(|_| invalid("无效网格"))?;
                if !(2..=128).contains(&size) { return Err(invalid("网格应在2到128之间")); }
                values.reserve(size * size * size);
            },
            "DOMAIN_MIN" | "DOMAIN_MAX" if tokens.len() == 4 && values.is_empty() => {
                let index = usize::from(command == "DOMAIN_MAX");
                if bounds[index] { return Err(invalid("重复输入范围")); }
                bounds[index] = true;
                let v = [number(tokens[1])?, number(tokens[2])?, number(tokens[3])?];
                if index == 0 { domain_min = v; } else { domain_max = v; }
            },
            "LUT_3D_INPUT_RANGE" if tokens.len() == 3 && values.is_empty() && bounds == [false; 2] => {
                domain_min.fill(number(tokens[1])?);
                domain_max.fill(number(tokens[2])?);
                bounds = [true; 2];
            },
            "LUT_1D_SIZE" | "LUT_1D_INPUT_RANGE" => return Err(invalid("当前仅支持独立3D LUT")),
            _ => {
                if size == 0 || tokens.len() != 3 || values.len() >= size * size * size {
                    return Err(invalid("采样行格式或数量错误"));
                }
                values.push([number(tokens[0])?, number(tokens[1])?, number(tokens[2])?]);
            },
        }
    }
    if size == 0 || values.len() != size * size * size
        || (0..3).any(|i| (domain_max[i] as f64 - domain_min[i] as f64) < 1e-12
            || !(domain_max[i] - domain_min[i]).is_finite()) {
        return Err(invalid("采样数量或输入范围错误"));
    }
    Ok(Lut3d { size, domain_min, domain_max, values, source_path: PathBuf::new(), sha256: String::new() })
}

impl Lut3d {
    pub(crate) fn apply(&self, samples: &mut [f32], strength: f32) {
        for pixel in samples.chunks_exact_mut(3) {
            let mut lo = [0usize; 3];
            let mut hi = [0usize; 3];
            let mut frac = [0.0; 3];
            for c in 0..3 {
                let p = ((pixel[c] - self.domain_min[c]) / (self.domain_max[c] - self.domain_min[c])).clamp(0.0, 1.0) * (self.size - 1) as f32;
                lo[c] = p as usize;
                hi[c] = (lo[c] + 1).min(self.size - 1);
                frac[c] = p - lo[c] as f32;
            }
            let mut output = [0.0; 3];
            for corner in 0..8 {
                let mut xyz = [0; 3];
                let mut weight = 1.0;
                for c in 0..3 {
                    let upper = corner & (1 << c) != 0;
                    xyz[c] = if upper { hi[c] } else { lo[c] };
                    weight *= if upper { frac[c] } else { 1.0 - frac[c] };
                }
                let sample = self.values[(xyz[2] * self.size + xyz[1]) * self.size + xyz[0]];
                for c in 0..3 { output[c] += sample[c] * weight; }
            }
            for c in 0..3 { pixel[c] = (pixel[c] + (output[c] - pixel[c]) * strength).clamp(0.0, 1.0); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_uses_red_fastest_and_strength() {
        let cube = "LUT_3D_SIZE 2\n0 0 0\n0 1 0\n1 0 0\n1 1 0\n0 0 1\n0 1 1\n1 0 1\n1 1 1";
        let lut = parse_cube(cube).unwrap();
        let mut p = [0.2, 0.7, 0.4];
        lut.apply(&mut p, 0.5);
        assert!((p[0] - 0.45).abs() < 1e-6);
        assert!((p[1] - 0.45).abs() < 1e-6);
        assert!((p[2] - 0.4).abs() < 1e-6);
        assert!(parse_cube("LUT_3D_SIZE 2\n0 0 0").is_err());
        assert!(parse_cube("LUT_1D_SIZE 2").is_err());
        assert!(parse_cube(&cube.replace("0 0 0", "NaN 0 0")).is_err());
        assert!(parse_cube(&format!("DOMAIN_MIN 1 1 1\nDOMAIN_MAX 0 0 0\n{cube}")).is_err());
    }
}
