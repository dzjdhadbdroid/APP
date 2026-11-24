//! Rendu des pages d'un PDF en images (moteur PDFium, embarqué dans l'exécutable Windows).

use std::path::{Path, PathBuf};

pub struct PageImage {
    pub width: usize,
    pub height: usize,
    /// Pixels RGBA (non prémultipliés), ligne par ligne.
    pub rgba: Vec<u8>,
}

#[cfg(all(windows, has_pdfium))]
static PDFIUM_DLL: &[u8] = include_bytes!("../assets/pdfium.dll");

/// Écrit la bibliothèque PDFium dans le dossier de données (une fois) et retourne son chemin.
#[cfg(all(windows, has_pdfium))]
fn library_path(data_dir: &Path) -> Result<PathBuf, String> {
    let dir = data_dir.join("pdfium");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("pdfium.dll");
    let up_to_date = std::fs::metadata(&path)
        .map(|m| m.len() == PDFIUM_DLL.len() as u64)
        .unwrap_or(false);
    if !up_to_date {
        std::fs::write(&path, PDFIUM_DLL).map_err(|e| e.to_string())?;
    }
    Ok(path)
}

#[cfg(not(all(windows, has_pdfium)))]
fn library_path(_data_dir: &Path) -> Result<PathBuf, String> {
    Err("l'affichage des PDF n'est pas inclus dans cette version (utilise « Ouvrir »)".into())
}

/// Rend les `max_pages` premières pages à `width_px` de large. Retourne aussi le nombre total de pages.
pub fn render_pages(
    data_dir: &Path,
    bytes: &[u8],
    width_px: i32,
    max_pages: usize,
) -> Result<(Vec<PageImage>, usize), String> {
    use pdfium_render::prelude::*;
    let lib = library_path(data_dir)?;
    let bindings =
        Pdfium::bind_to_library(&lib).map_err(|e| format!("PDFium indisponible : {e}"))?;
    let pdfium = Pdfium::new(bindings);
    let doc = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .map_err(|e| format!("PDF illisible : {e}"))?;
    let total = doc.pages().len() as usize;
    let config = PdfRenderConfig::new().set_target_width(width_px);
    let mut out = Vec::new();
    for page in doc.pages().iter().take(max_pages) {
        let bitmap = page
            .render_with_config(&config)
            .map_err(|e| format!("rendu impossible : {e}"))?;
        let (w, h) = (bitmap.width() as usize, bitmap.height() as usize);
        let raw: Vec<u8> = bitmap.as_raw_bytes().to_vec();
        if raw.len() < w * h * 4 {
            return Err("image de page inattendue".into());
        }
        // PDFium fournit du BGRA : on le convertit en RGBA.
        let mut rgba = Vec::with_capacity(w * h * 4);
        for px in raw.chunks_exact(4).take(w * h) {
            rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
        out.push(PageImage {
            width: w,
            height: h,
            rgba,
        });
    }
    Ok((out, total))
}
