//! Recommendation score. Higher means more relevant to the focus area.
//!
//! The stored catalog `score` is only one input (source quality). The number
//! printed by `recommend` also uses similarity to the area seed, keyword
//! strength, shelf life, recency, and the like/dislike profile. Ordering never
//! uses the video id.

#[derive(Clone, Debug)]
pub struct RankInput<'a> {
    pub affinity: i32,
    pub seed_cosine: Option<f64>,
    pub like_cosine: Option<f64>,
    pub dislike_cosine: Option<f64>,
    pub source_score: f64,
    pub recommenders: i64,
    pub shelf_life: &'a str,
    pub published: &'a str,
}

/// Continuous score in roughly the 0–10 range, rounded to 3 decimals.
pub fn recommendation_score(input: &RankInput<'_>) -> f64 {
    let keyword = (input.affinity as f64 / 3.0).clamp(0.0, 1.0);
    let seed = input.seed_cosine.unwrap_or(0.0).clamp(0.0, 1.0);
    let quality = source_quality(input.source_score, input.recommenders);
    let fresh = freshness(input.shelf_life, input.published);
    let profile = profile_term(input.like_cosine, input.dislike_cosine);
    let seed_weight = if profile.is_some() { 0.40 } else { 0.50 };
    let profile_weight = if profile.is_some() { 0.10 } else { 0.0 };
    let profile_value = profile.map(|value| (value + 1.0) / 2.0).unwrap_or(0.0);
    let raw = seed_weight * seed
        + 0.24 * keyword
        + 0.16 * quality
        + 0.10 * fresh
        + profile_weight * profile_value;
    (raw * 10_000.0).round() / 1_000.0
}

pub fn compare_rank(left: &RankParts, right: &RankParts) -> std::cmp::Ordering {
    right
        .score
        .partial_cmp(&left.score)
        .unwrap_or(std::cmp::Ordering::Equal)
        .then_with(|| right.affinity.cmp(&left.affinity))
        .then_with(|| {
            right
                .seed_cosine
                .partial_cmp(&left.seed_cosine)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .then_with(|| {
            right
                .quality
                .partial_cmp(&left.quality)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .then_with(|| {
            right
                .fresh
                .partial_cmp(&left.fresh)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .then_with(|| left.title.cmp(&right.title))
}

#[derive(Clone, Debug)]
pub struct RankParts {
    pub score: f64,
    pub affinity: i32,
    pub seed_cosine: f64,
    pub quality: f64,
    pub fresh: f64,
    pub title: String,
}

fn source_quality(score: f64, recommenders: i64) -> f64 {
    let from_score = (score / 15.0).clamp(0.0, 1.0);
    let from_people = (recommenders as f64 / 8.0).clamp(0.0, 1.0);
    0.7 * from_score + 0.3 * from_people
}

fn freshness(shelf_life: &str, published: &str) -> f64 {
    let base = match shelf_life {
        "evergreen" => 0.8_f64,
        "dated" => 0.4,
        "news" => 0.15,
        _ => 0.55,
    };
    let year: i32 = published
        .get(0..4)
        .and_then(|text| text.parse().ok())
        .unwrap_or(0);
    let bump = if year >= 2025 {
        0.15
    } else if year >= 2023 {
        0.08
    } else {
        0.0
    };
    (base + bump).clamp(0.0, 1.0)
}

/// Like similarity minus dislike similarity, in [-1, 1]. None when the user
/// has no embedded likes or dislikes.
fn profile_term(like: Option<f64>, dislike: Option<f64>) -> Option<f64> {
    if like.is_none() && dislike.is_none() {
        return None;
    }
    Some((like.unwrap_or(0.0) - dislike.unwrap_or(0.0)).clamp(-1.0, 1.0))
}

pub fn source_quality_of(score: f64, recommenders: i64) -> f64 {
    source_quality(score, recommenders)
}

pub fn freshness_of(shelf_life: &str, published: &str) -> f64 {
    freshness(shelf_life, published)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(score: f64, recommenders: i64, shelf: &str) -> RankInput<'_> {
        RankInput {
            affinity: 3,
            seed_cosine: None,
            like_cosine: None,
            dislike_cosine: None,
            source_score: score,
            recommenders,
            shelf_life: shelf,
            published: "",
        }
    }

    #[test]
    fn higher_quality_outranks_an_id_that_sorts_first() {
        let weak = recommendation_score(&input(1.0, 1, "dated"));
        let strong = recommendation_score(&input(14.0, 9, "evergreen"));
        assert!(strong > weak);
        let mut rows = [
            ("-0000000000", weak, "thin"),
            ("zzzzzzzzzzz", strong, "harness"),
        ];
        rows.sort_by(|left, right| {
            compare_rank(
                &RankParts {
                    score: left.1,
                    affinity: 3,
                    seed_cosine: 0.0,
                    quality: 0.0,
                    fresh: 0.0,
                    title: left.2.into(),
                },
                &RankParts {
                    score: right.1,
                    affinity: 3,
                    seed_cosine: 0.0,
                    quality: 0.0,
                    fresh: 0.0,
                    title: right.2.into(),
                },
            )
        });
        assert_eq!(rows[0].0, "zzzzzzzzzzz");
        let ids: Vec<&str> = rows.iter().map(|row| row.0).collect();
        let mut alpha = ids.clone();
        alpha.sort_unstable();
        assert_ne!(ids, alpha);
        assert!(format!("{strong}").contains('.'));
    }

    #[test]
    fn equal_scores_break_on_title_not_position() {
        let parts = |title: &str| RankParts {
            score: 4.2,
            affinity: 3,
            seed_cosine: 0.5,
            quality: 0.4,
            fresh: 0.8,
            title: title.into(),
        };
        assert_eq!(
            compare_rank(&parts("Zebra"), &parts("Alpha")),
            std::cmp::Ordering::Greater
        );
    }
}
