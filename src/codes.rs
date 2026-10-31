//! QR codes and barcodes found in a picture, read entirely on this computer.

use crate::pixels::PixelBuffer;
use rxing::BarcodeFormat;

/// One code read from a picture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoundCode {
    /// The code's kind as people know it: "QR Code", "EAN-13"…
    pub kind: &'static str,
    pub text: String,
}

impl FoundCode {
    /// The text is a web address that is safe to hand to the browser.
    pub fn is_web_link(&self) -> bool {
        is_web_link(&self.text)
    }
}

/// Only http and https: other schemes (file:, ms-settings:, javascript:…)
/// could run something on this computer.
pub fn is_web_link(text: &str) -> bool {
    let t = text.trim();
    let lower = t.get(..8).unwrap_or(t).to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://"))
        && !t.chars().any(|c| c.is_whitespace() || c.is_control())
}

fn kind_name(format: &BarcodeFormat) -> &'static str {
    match format {
        BarcodeFormat::QR_CODE => "QR Code",
        BarcodeFormat::MICRO_QR_CODE => "Micro QR",
        BarcodeFormat::RECTANGULAR_MICRO_QR_CODE => "rMQR",
        BarcodeFormat::AZTEC => "Aztec",
        BarcodeFormat::DATA_MATRIX => "Data Matrix",
        BarcodeFormat::PDF_417 => "PDF417",
        BarcodeFormat::MAXICODE => "MaxiCode",
        BarcodeFormat::EAN_8 => "EAN-8",
        BarcodeFormat::EAN_13 => "EAN-13",
        BarcodeFormat::UPC_A => "UPC-A",
        BarcodeFormat::UPC_E => "UPC-E",
        BarcodeFormat::CODE_39 => "Code 39",
        BarcodeFormat::CODE_93 => "Code 93",
        BarcodeFormat::CODE_128 => "Code 128",
        BarcodeFormat::CODABAR => "Codabar",
        BarcodeFormat::ITF => "ITF",
        BarcodeFormat::RSS_14 => "GS1 DataBar",
        BarcodeFormat::RSS_EXPANDED => "GS1 DataBar Expanded",
        _ => "Barcode",
    }
}

/// Grey levels of `img`; see-through pixels (outside a lasso) count as white.
fn luma(img: &PixelBuffer) -> Vec<u8> {
    img.bgra
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            let (b, g, r, a) = (p[0] as u32, p[1] as u32, p[2] as u32, p[3] as u32);
            let y = (299 * r + 587 * g + 114 * b) / 1000;
            ((y * a + 255 * (255 - a)) / 255) as u8
        })
        .collect()
}

/// Every QR code and barcode in `img`, each text once, top to bottom.
/// Light codes on a dark background (common on dark pages and branded
/// codes) are read too, from the picture with its greys inverted.
pub fn scan(img: &PixelBuffer) -> Vec<FoundCode> {
    let (w, h) = (img.width(), img.height());
    if w <= 0 || h <= 0 {
        return Vec::new();
    }
    let grey = luma(img);
    let inverted: Vec<u8> = grey.iter().map(|v| 255 - v).collect();
    let results = [grey, inverted].into_iter().flat_map(|l| {
        rxing::helpers::detect_multiple_in_luma(l, w as u32, h as u32).unwrap_or_default()
    });
    let mut found: Vec<(f32, FoundCode)> = Vec::new();
    for r in results {
        let text = r.getText().trim().to_string();
        if text.is_empty() || found.iter().any(|(_, f)| f.text == text) {
            continue;
        }
        let top = r.getRXingResultPoints().iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
        found.push((top, FoundCode { kind: kind_name(r.getBarcodeFormat()), text }));
    }
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    found.into_iter().map(|(_, f)| f).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::PixelRect;
    use rxing::Writer;

    /// `text` encoded as `format`, drawn black on white at (x, y) inside a
    /// white picture of `size`, `scale` pixels per module.
    fn picture_with(
        codes: &[(&str, BarcodeFormat, i32, i32)],
        size: (i32, i32),
        scale: u32,
    ) -> PixelBuffer {
        let mut img = PixelBuffer::solid(PixelRect::new(0, 0, size.0, size.1), 255, 255, 255);
        for (text, format, x, y) in codes {
            let m = rxing::MultiFormatWriter.encode(text, format, 0, 0).expect("encodes");
            for my in 0..m.getHeight() {
                for mx in 0..m.getWidth() {
                    if !m.get(mx, my) {
                        continue;
                    }
                    // 1D codes come back one module tall: stretch them.
                    let rows = if m.getHeight() == 1 { 60 } else { scale };
                    for dy in 0..rows {
                        for dx in 0..scale {
                            let px = x + (mx * scale + dx) as i32;
                            let py = y + (my * rows.max(scale) + dy) as i32;
                            img.put_pixel(
                                crate::geometry::PixelPoint { x: px, y: py },
                                [0, 0, 0, 255],
                            );
                        }
                    }
                }
            }
        }
        img
    }

    #[test]
    fn reads_a_qr_code() {
        let img = picture_with(
            &[("https://example.com/a", BarcodeFormat::QR_CODE, 40, 40)],
            (300, 300),
            4,
        );
        assert_eq!(
            scan(&img),
            vec![FoundCode { kind: "QR Code", text: "https://example.com/a".into() }]
        );
    }

    #[test]
    fn reads_a_light_qr_code_on_a_dark_background() {
        let mut img = picture_with(
            &[("https://example.com/dark", BarcodeFormat::QR_CODE, 40, 40)],
            (300, 300),
            4,
        );
        for p in img.bgra.as_chunks_mut::<4>().0 {
            let v = 255 - p[0];
            p.copy_from_slice(&[v, v, v, 255]);
        }
        assert_eq!(scan(&img).first().map(|f| f.text.as_str()), Some("https://example.com/dark"));
    }

    #[test]
    fn reads_an_ean13_barcode() {
        let img = picture_with(&[("5901234123457", BarcodeFormat::EAN_13, 30, 30)], (400, 140), 2);
        assert_eq!(scan(&img), vec![FoundCode { kind: "EAN-13", text: "5901234123457".into() }]);
    }

    #[test]
    fn reads_a_code_128_barcode() {
        let img = picture_with(&[("GYOZA-0042", BarcodeFormat::CODE_128, 30, 30)], (400, 140), 2);
        assert_eq!(scan(&img), vec![FoundCode { kind: "Code 128", text: "GYOZA-0042".into() }]);
    }

    #[test]
    fn reads_several_codes_top_to_bottom() {
        let img = picture_with(
            &[
                ("second", BarcodeFormat::QR_CODE, 60, 300),
                ("first", BarcodeFormat::QR_CODE, 60, 30),
            ],
            (300, 560),
            4,
        );
        let texts: Vec<_> = scan(&img).into_iter().map(|f| f.text).collect();
        assert_eq!(texts, ["first", "second"]);
    }

    #[test]
    fn same_text_twice_is_listed_once() {
        let img = picture_with(
            &[
                ("twice", BarcodeFormat::QR_CODE, 30, 30),
                ("twice", BarcodeFormat::QR_CODE, 300, 30),
            ],
            (560, 300),
            4,
        );
        assert_eq!(scan(&img).len(), 1);
    }

    #[test]
    fn a_plain_picture_has_no_codes() {
        let img = PixelBuffer::solid(PixelRect::new(0, 0, 200, 120), 40, 90, 200);
        assert!(scan(&img).is_empty());
        let empty = PixelBuffer::new(PixelRect::new(0, 0, 0, 0), Vec::new());
        assert!(scan(&empty).is_empty());
    }

    #[test]
    fn see_through_pixels_read_as_white() {
        let mut img = picture_with(&[("lasso", BarcodeFormat::QR_CODE, 40, 40)], (300, 300), 4);
        // Outside a lasso: transparent, colour bytes zero (black if alpha were ignored).
        for p in img.bgra.as_chunks_mut::<4>().0 {
            if p[0] == 255 {
                p.copy_from_slice(&[0, 0, 0, 0]);
            }
        }
        assert_eq!(scan(&img).first().map(|f| f.text.as_str()), Some("lasso"));
    }

    #[test]
    fn only_http_and_https_are_web_links() {
        assert!(is_web_link("https://example.com"));
        assert!(is_web_link("HTTP://EXAMPLE.COM/x?y=1"));
        assert!(!is_web_link("file:///C:/Windows/System32/calc.exe"));
        assert!(!is_web_link("ms-settings:privacy"));
        assert!(!is_web_link("javascript:alert(1)"));
        assert!(!is_web_link("www.example.com"));
        assert!(!is_web_link("https://example.com and more"));
        assert!(!is_web_link("http"));
        assert!(!is_web_link("httpè://x"));
    }
}
