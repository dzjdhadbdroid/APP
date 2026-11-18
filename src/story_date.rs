//! Dates de l'univers fictif : "1998", "1998-06" ou "1998-06-12" → clé de tri numérique.
//! Calendrier simplifié (12 mois × 31 jours) : seul l'ordre compte, pas la durée réelle.

pub fn sort_key(s: &str) -> Option<i64> {
    let parts: Vec<&str> = s.trim().split('-').collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let year: i64 = parts[0].parse().ok()?;
    let month: i64 = parts.get(1).map_or(Some(1), |m| m.parse().ok())?;
    let day: i64 = parts.get(2).map_or(Some(1), |d| d.parse().ok())?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(year * 372 + (month - 1) * 31 + (day - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_partial_dates() {
        assert!(sort_key("1998").is_some());
        assert!(sort_key("1998-06").is_some());
        assert!(sort_key("1998-06-12").is_some());
    }

    #[test]
    fn keeps_chronological_order() {
        assert!(sort_key("1998-06-12") < sort_key("1998-06-13"));
        assert!(sort_key("1998-12-31") < sort_key("1999-01-01"));
        assert!(sort_key("1998") <= sort_key("1998-01-01"));
    }

    #[test]
    fn rejects_garbage() {
        for bad in ["", "abc", "1998-13", "1998-06-32", "1-2-3-4", "1998-xx"] {
            assert_eq!(sort_key(bad), None, "{bad}");
        }
    }
}
