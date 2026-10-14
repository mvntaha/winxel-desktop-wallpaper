use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

pub const FB_W: usize = 960;
pub const FB_H: usize = 540;

pub struct Image {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u32>,
}

impl Image {
    pub fn load_png(path: &Path) -> Result<Image, String> {
        let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut dec = png::Decoder::new(BufReader::new(file));
        dec.set_transformations(png::Transformations::normalize_to_color8());
        let mut reader = dec.read_info().map_err(|e| format!("{}: {e}", path.display()))?;
        let mut buf = vec![0u8; reader.output_buffer_size().ok_or("png too large")?];
        let info = reader.next_frame(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
        let (w, h) = (info.width as usize, info.height as usize);
        let ch = match info.color_type {
            png::ColorType::Grayscale => 1,
            png::ColorType::GrayscaleAlpha => 2,
            png::ColorType::Rgb => 3,
            png::ColorType::Rgba => 4,
            png::ColorType::Indexed => return Err(format!("{}: unexpanded palette", path.display())),
        };
        let px = buf[..w * h * ch]
            .chunks_exact(ch)
            .map(|p| {
                let (r, g, b, a) = match ch {
                    1 => (p[0], p[0], p[0], 255),
                    2 => (p[0], p[0], p[0], p[1]),
                    3 => (p[0], p[1], p[2], 255),
                    _ => (p[0], p[1], p[2], p[3]),
                };
                u32::from_le_bytes([b, g, r, a])
            })
            .collect();
        Ok(Image { w, h, px })
    }

    pub fn save_png(&self, path: &Path) -> Result<(), String> {
        let file = File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), self.w as u32, self.h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let rgba: Vec<u8> = self
            .px
            .iter()
            .flat_map(|p| {
                let [b, g, r, a] = p.to_le_bytes();
                [r, g, b, a]
            })
            .collect();
        enc.write_header()
            .and_then(|mut w| w.write_image_data(&rgba))
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn placeholder() -> Image {
        let mut px = Vec::with_capacity(FB_W * FB_H);
        for y in 0..FB_H {
            for x in 0..FB_W {
                let sky = (40 + y * 60 / FB_H) as u8;
                let check = ((x / 12 + y / 12) % 2) as u8 * 10;
                px.push(u32::from_le_bytes([sky + 70 + check, sky + 20 + check, sky + check, 255]));
            }
        }
        Image { w: FB_W, h: FB_H, px }
    }
}

pub fn assets_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().skip(1).map(|d| d.join("assets")).find(|d| d.is_dir())
}
