//! Générateur PDF minimal, sans dépendance : texte seulement (Helvetica, accents français),
//! titres, listes, retour à la ligne et pagination automatiques. Logique pure, testable seule.

const PAGE_W: f32 = 595.0;
const PAGE_H: f32 = 842.0;
const MARGIN_X: f32 = 56.0;
const MARGIN_TOP: f32 = 64.0;
const MARGIN_BOTTOM: f32 = 64.0;
const BODY: f32 = 11.0;

struct Line {
    text: String,
    size: f32,
    bold: bool,
    gap_before: f32,
}

/// Largeur approximative d'un caractère, en fraction du corps (Helvetica).
fn char_em(c: char) -> f32 {
    match c {
        ' ' => 0.28,
        'i' | 'l' | 'j' | 't' | 'f' | 'I' | '.' | ',' | ':' | ';' | '\'' | '!' | '|' | '’' => 0.3,
        'm' | 'w' | 'M' | 'W' | '—' => 0.85,
        c if c.is_uppercase() => 0.7,
        c if c.is_ascii_digit() => 0.56,
        _ => 0.53,
    }
}

fn text_width(s: &str, size: f32, bold: bool) -> f32 {
    let em: f32 = s.chars().map(char_em).sum();
    em * size * if bold { 1.08 } else { 1.0 }
}

/// Octet WinAnsi (CP1252) d'un caractère, ou `None` s'il n'existe pas dans la police.
fn winansi(c: char) -> Option<u8> {
    let u = c as u32;
    Some(match c {
        '€' => 0x80,
        '…' => 0x85,
        '‘' => 0x91,
        '’' => 0x92,
        '“' => 0x93,
        '”' => 0x94,
        '•' => 0x95,
        '–' => 0x96,
        '—' => 0x97,
        'œ' => 0x9C,
        'Œ' => 0x8C,
        _ if (0x20..0x7F).contains(&u) || (0xA0..=0xFF).contains(&u) => u as u8,
        _ => return None,
    })
}

/// Remplacements textuels pour les caractères absents de la police.
fn substitute(c: char) -> Option<&'static str> {
    Some(match c {
        '→' => "->",
        '←' => "<-",
        '⇄' => "<->",
        '●' | '·' => "-",
        '\t' => "    ",
        '\u{a0}' => " ",
        _ => return None,
    })
}

fn clean(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if winansi(c).is_some() && c != '\u{a0}' {
            out.push(c);
        } else if let Some(r) = substitute(c) {
            out.push_str(r);
        } else if !c.is_control() {
            out.push('?');
        }
    }
    out
}

/// PDF string literal : octets non ASCII et caractères spéciaux en échappements octaux.
fn pdf_string(s: &str) -> String {
    let mut out = String::from("(");
    for c in clean(s).chars() {
        let b = winansi(c).unwrap_or(b'?');
        match b {
            b'(' | b')' | b'\\' => {
                out.push('\\');
                out.push(b as char);
            }
            0x20..=0x7E => out.push(b as char),
            _ => out.push_str(&format!("\\{b:03o}")),
        }
    }
    out.push(')');
    out
}

/// Chaîne des métadonnées (encodage PDFDocEncoding, différent de WinAnsi) : on ne garde que
/// l'ASCII et le Latin-1, communs aux deux ; le reste devient « - ».
fn info_string(s: &str) -> String {
    let mut out = String::from("(");
    for c in s.chars() {
        let u = c as u32;
        match c {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            _ if (0x20..0x7F).contains(&u) => out.push(c),
            _ if (0xA0..=0xFF).contains(&u) => out.push_str(&format!("\\{u:03o}")),
            _ => out.push('-'),
        }
    }
    out.push(')');
    out
}

fn wrap(text: &str, size: f32, bold: bool, max_w: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        // Mot plus large qu'une ligne : coupé en morceaux.
        let mut word = word.to_string();
        while text_width(&word, size, bold) > max_w {
            let mut cut = String::new();
            for c in word.chars() {
                if text_width(&format!("{cut}{c}"), size, bold) > max_w {
                    break;
                }
                cut.push(c);
            }
            if cut.is_empty() {
                break;
            }
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
            }
            lines.push(cut.clone());
            word = word[cut.len()..].to_string();
        }
        let candidate = if cur.is_empty() {
            word.clone()
        } else {
            format!("{cur} {word}")
        };
        if text_width(&candidate, size, bold) <= max_w {
            cur = candidate;
        } else {
            lines.push(std::mem::take(&mut cur));
            cur = word;
        }
    }
    if !cur.is_empty() || lines.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Retire les marques Markdown de mise en forme en ligne (`**gras**`, `*italique*`).
fn strip_inline(s: &str) -> String {
    s.replace("**", "").replace('*', "")
}

fn layout(title: &str, markdown: &str) -> Vec<Line> {
    let max_w = PAGE_W - 2.0 * MARGIN_X;
    let mut lines = Vec::new();
    for l in wrap(title, 24.0, true, max_w) {
        lines.push(Line {
            text: l,
            size: 24.0,
            bold: true,
            gap_before: 0.0,
        });
    }
    let mut pending_gap: f32 = 10.0;
    for raw in markdown.lines() {
        let raw = raw.trim_end();
        if raw.trim().is_empty() {
            pending_gap = pending_gap.max(5.0);
            continue;
        }
        let (text, size, bold, gap) = if let Some(t) = raw.strip_prefix("### ") {
            (t.to_string(), 13.0, true, 8.0)
        } else if let Some(t) = raw.strip_prefix("## ") {
            (t.to_string(), 16.0, true, 12.0)
        } else if let Some(t) = raw.strip_prefix("# ") {
            (t.to_string(), 20.0, true, 14.0)
        } else {
            (strip_inline(raw), BODY, false, 0.0)
        };
        for (i, l) in wrap(&text, size, bold, max_w).into_iter().enumerate() {
            lines.push(Line {
                text: l,
                size,
                bold,
                gap_before: if i == 0 { pending_gap.max(gap) } else { 0.0 },
            });
            pending_gap = 0.0;
        }
        pending_gap = 0.0;
    }
    lines
}

pub fn markdown_to_pdf(title: &str, markdown: &str) -> Vec<u8> {
    // ── pagination : chaque page = liste de (x, y, ligne) ──
    let mut pages: Vec<Vec<(f32, &Line)>> = vec![Vec::new()];
    let lines = layout(title, markdown);
    let mut y = PAGE_H - MARGIN_TOP;
    for line in &lines {
        let leading = line.size * 1.3;
        let need = if pages.last().is_some_and(|p| p.is_empty()) {
            leading
        } else {
            line.gap_before + leading
        };
        if y - need < MARGIN_BOTTOM && !pages.last().unwrap().is_empty() {
            pages.push(Vec::new());
            y = PAGE_H - MARGIN_TOP;
        }
        if !pages.last().unwrap().is_empty() {
            y -= line.gap_before;
        }
        y -= leading;
        pages.last_mut().unwrap().push((y, line));
    }

    // ── objets ──
    // 1 catalogue · 2 pages · 3 Helvetica · 4 Helvetica-Bold · 5 infos · puis (page, contenu) × n
    let n_pages = pages.len();
    let total_objs = 5 + 2 * n_pages;
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<usize> = Vec::with_capacity(total_objs);
    let mut push_obj = |out: &mut Vec<u8>, offsets: &mut Vec<usize>, body: &[u8]| {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", offsets.len()).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };

    push_obj(&mut out, &mut offsets, b"<< /Type /Catalog /Pages 2 0 R >>");
    let kids: Vec<String> = (0..n_pages).map(|i| format!("{} 0 R", 6 + 2 * i)).collect();
    push_obj(
        &mut out,
        &mut offsets,
        format!(
            "<< /Type /Pages /Count {n_pages} /Kids [{}] >>",
            kids.join(" ")
        )
        .as_bytes(),
    );
    push_obj(
        &mut out,
        &mut offsets,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
    );
    push_obj(
        &mut out,
        &mut offsets,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>",
    );
    push_obj(
        &mut out,
        &mut offsets,
        format!(
            "<< /Title {} /Producer (Sitcom Studio) >>",
            info_string(title)
        )
        .as_bytes(),
    );

    for (i, page) in pages.iter().enumerate() {
        let content_obj = 7 + 2 * i;
        push_obj(
            &mut out,
            &mut offsets,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}] \
                 /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {content_obj} 0 R >>"
            )
            .as_bytes(),
        );
        let mut stream = String::new();
        for (y, line) in page {
            if line.text.is_empty() {
                continue;
            }
            let font = if line.bold { "F2" } else { "F1" };
            stream.push_str(&format!(
                "BT /{font} {} Tf 1 0 0 1 {MARGIN_X} {:.2} Tm {} Tj ET\n",
                line.size,
                y,
                pdf_string(&line.text)
            ));
        }
        let mut body =
            format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()).into_bytes();
        body.shrink_to_fit();
        push_obj(&mut out, &mut offsets, &body);
    }

    // ── table des références croisées ──
    let xref_at = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", total_objs + 1).as_bytes());
    for off in &offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            total_objs + 1
        )
        .as_bytes(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn as_text(pdf: &[u8]) -> String {
        pdf.iter().map(|&b| b as char).collect()
    }

    fn find_last(hay: &[u8], needle: &[u8]) -> usize {
        (0..=hay.len() - needle.len())
            .rev()
            .find(|&i| &hay[i..i + needle.len()] == needle)
            .unwrap()
    }

    #[test]
    fn structure_is_valid_and_xref_offsets_are_exact() {
        let pdf = markdown_to_pdf("Titre — é", "# A\n\nTexte **gras** ici.\n- point\n");
        assert!(pdf.starts_with(b"%PDF-1.4") && pdf.ends_with(b"%%EOF\n"));
        // On travaille sur les octets : l'en-tête contient des octets > 127.
        let k = find_last(&pdf, b"startxref\n") + b"startxref\n".len();
        let start: usize = std::str::from_utf8(&pdf[k..])
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(pdf[start..].starts_with(b"xref\n"));
        let tail = std::str::from_utf8(&pdf[start..]).unwrap();
        let entries: Vec<&str> = tail
            .lines()
            .skip(3)
            .take_while(|l| l.ends_with(" n "))
            .collect();
        assert_eq!(entries.len(), 7); // catalogue, pages, 2 polices, infos, page, contenu
        for (i, e) in entries.iter().enumerate() {
            let off: usize = e[..10].parse().unwrap();
            assert!(
                pdf[off..].starts_with(format!("{} 0 obj", i + 1).as_bytes()),
                "objet {} mal placé",
                i + 1
            );
        }
    }

    #[test]
    fn long_text_makes_several_pages_and_lines_fit() {
        let md = (0..400)
            .map(|i| format!("Ligne numéro {i} avec assez de mots pour tenir sur la page."))
            .collect::<Vec<_>>()
            .join("\n");
        let t = as_text(&markdown_to_pdf("Long", &md));
        let count: usize = t
            .split("/Count ")
            .nth(1)
            .unwrap()
            .split(' ')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(count > 1);
        for l in layout("Long", &md) {
            assert!(
                text_width(&l.text, l.size, l.bold) <= PAGE_W - 2.0 * MARGIN_X + 0.01,
                "{}",
                l.text
            );
        }
    }

    #[test]
    fn a_huge_unbreakable_word_is_split() {
        let word = "x".repeat(500);
        for l in layout("t", &word) {
            assert!(text_width(&l.text, l.size, l.bold) <= PAGE_W - 2.0 * MARGIN_X + 0.01);
        }
    }

    #[test]
    fn accents_and_special_characters_are_encoded() {
        assert_eq!(winansi('é'), Some(0xE9));
        assert_eq!(winansi('’'), Some(0x92));
        assert_eq!(winansi('«'), Some(0xAB));
        assert_eq!(winansi('→'), None);
        assert_eq!(clean("a → b"), "a -> b");
        assert_eq!(clean("😀"), "?");
        assert_eq!(pdf_string("a(b)\\é"), "(a\\(b\\)\\\\\\351)");
    }

    #[test]
    fn info_title_uses_a_safe_subset() {
        assert_eq!(
            info_string("Ma sitcom — bible é"),
            "(Ma sitcom - bible \\351)"
        );
    }

    #[test]
    fn empty_input_still_gives_a_valid_one_page_pdf() {
        let t = as_text(&markdown_to_pdf("", ""));
        assert!(t.contains("/Count 1"));
    }
}
