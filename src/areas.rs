//! Focus areas. Lexical rules plus optional cosine similarity to a seed description.
//!
//! Embedding similarity can promote a video into an area. It cannot override a hard block,
//! so game demos such as hide-and-seek stay out of agentic engineering.

use std::sync::LazyLock;

use regex::Regex;

pub const AREA_IDS: [&str; 3] = ["agentic-engineering", "ai-engineering", "ai-research"];

pub const SEEDS: [&str; 3] = [
    "Coding agents and agent harnesses for software engineering: Model Context Protocol, MCP, tool calling, context engineering, sandboxes, evals of coding agents, multi-agent software systems, Claude Code, agent skills, SWE-bench, IDE agents.",
    "Applied AI engineering: large language models, retrieval-augmented generation, fine-tuning, inference servers, prompt engineering, production LLM systems, vector search, evaluation harnesses for models.",
    "AI research: alignment, AI safety, scaling laws, neural network lectures, backpropagation, attention, deep learning theory, papers and arxiv talks.",
];

static HARD_BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)hide[- ]and[- ]seek|stone axe|transformer home|electrical transformer|ableton|academy award|hyperland").unwrap()
});
static SOFTWARE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)coding|harness|\bmcp\b|claude|agentic|sandbox|swe-bench|context engineering|tool calling|vibe|software engineer|\brepo\b|repository").unwrap()
});
static ML_PHRASE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)language model|\bllm\b|chatgpt|gpt-4|gpt-3|transformer|neural network|deep learning|diffusion|fine-?tun|retrieval-augmented|\brag\b|backprop").unwrap()
});

const AGENTIC_SLUGS: &[&str] = &[
    "coding-agents",
    "agent-engineering",
    "model-context-protocol",
    "harness-engineering",
    "agent-memory",
    "agent-skills",
    "agent-orchestration",
    "multi-agent-systems",
    "computer-use",
    "workflow-automation",
    "software-factories",
    "context-engineering",
    "sandboxes-and-execution-isolation",
];
const ENGINEERING_SLUGS: &[&str] = &[
    "rag-and-knowledge",
    "evals",
    "fine-tuning",
    "inference",
    "local-inference",
    "search-and-retrieval",
    "structured-outputs",
    "observability-and-reliability",
    "developer-workflows-and-testing",
    "ai-cost-and-performance-engineering",
    "post-training",
    "voice-ai",
];
const RESEARCH_SLUGS: &[&str] = &[
    "safety-and-governance",
    "machine-learning-fundamentals",
    "ai-security",
    "data-quality-and-curation",
];
const AGENTIC_STRONG: &[&str] = &[
    "coding agent",
    "claude code",
    "vibe coding",
    "swe-agent",
    "swe-bench",
    "agent harness",
    "harness engineering",
    "model context protocol",
    "context engineering",
    "agentic",
    "llm agent",
    "agent skill",
    "tool calling",
];
const ENGINEERING_STRONG: &[&str] = &[
    "large language model",
    "language model",
    "chatgpt",
    "retrieval-augmented",
    "prompt engineering",
    "fine-tuning",
    "fine tuning",
];
const RESEARCH_STRONG: &[&str] = &[
    "ai safety",
    "alignment",
    "scaling law",
    "attention is all you need",
    "backpropagation",
];

#[derive(Clone, Debug)]
pub struct Signals<'a> {
    pub title: &'a str,
    pub summary: &'a str,
    pub why: &'a str,
    pub topic: &'a str,
    pub tags: &'a str,
    pub topic_slugs: &'a [String],
    pub categories: &'a [String],
    pub content_type: &'a str,
    /// Cosine similarity to the three area seeds, when the embedder is available.
    pub cosines: Option<[f32; 3]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Affinity {
    pub agentic: i32,
    pub engineering: i32,
    pub research: i32,
}

impl Affinity {
    pub fn get(self, area: &str) -> i32 {
        match area {
            "agentic-engineering" => self.agentic,
            "ai-engineering" => self.engineering,
            "ai-research" => self.research,
            _ => 0,
        }
    }

    pub fn column(area: &str) -> Option<&'static str> {
        match area {
            "agentic-engineering" => Some("area_agentic"),
            "ai-engineering" => Some("area_engineering"),
            "ai-research" => Some("area_research"),
            _ => None,
        }
    }
}

pub fn classify(video: &Signals<'_>) -> Affinity {
    if matches!(video.content_type, "comedy" | "music") || hard_block(video) {
        return Affinity {
            agentic: 0,
            engineering: 0,
            research: 0,
        };
    }
    let blob = blob(video);
    let mut affinity = Affinity {
        agentic: lexical_agentic(video, &blob),
        engineering: lexical_engineering(video, &blob),
        research: lexical_research(video, &blob),
    };
    if let Some(cosines) = video.cosines {
        affinity.agentic = promote(affinity.agentic, cosines[0]);
        affinity.engineering = promote(affinity.engineering, cosines[1]);
        affinity.research = promote(affinity.research, cosines[2]);
    }
    affinity
}

fn promote(lexical: i32, cosine: f32) -> i32 {
    if lexical >= 2 {
        return lexical;
    }
    // Seed cosine alone sits high for ordinary programming talks (the agentic
    // seed mentions software engineering). Similarity confirms a keyword hit;
    // it does not pull a video in by itself.
    if lexical >= 1 && cosine >= 0.50 {
        lexical.max(2)
    } else {
        lexical
    }
}

fn hard_block(video: &Signals<'_>) -> bool {
    HARD_BLOCK.is_match(video.title)
        || HARD_BLOCK.is_match(video.topic)
        || HARD_BLOCK.is_match(video.tags)
        || HARD_BLOCK.is_match(video.summary)
}

fn blob(video: &Signals<'_>) -> String {
    format!(
        "{} {} {} {} {} {} {}",
        video.title,
        video.summary,
        video.why,
        video.topic,
        video.tags,
        video.topic_slugs.join(" "),
        video.categories.join(" ")
    )
    .to_lowercase()
}

fn has_slug(video: &Signals<'_>, slugs: &[&str]) -> bool {
    video
        .topic_slugs
        .iter()
        .any(|slug| slugs.contains(&slug.as_str()))
}

fn has_category(video: &Signals<'_>, id: &str) -> bool {
    video.categories.iter().any(|category| category == id)
}

fn phrase(blob: &str, phrases: &[&str]) -> bool {
    phrases.iter().any(|phrase| blob.contains(phrase))
}

fn lexical_agentic(video: &Signals<'_>, blob: &str) -> i32 {
    let mut score = 0;
    if has_slug(video, AGENTIC_SLUGS) || phrase(blob, AGENTIC_STRONG) {
        score = 3;
    }
    let contextual = blob.contains("multi-agent")
        || blob.contains("tool use")
        || blob.contains("ai agent")
        || blob.contains("function calling");
    if contextual && SOFTWARE.is_match(blob) {
        score = score.max(2);
    }
    // "agent-based" is simulation vocabulary (ants, slime mold), not a coding agent.
    let weak = blob.replace("agent-based", " ");
    if score == 0
        && (weak.contains("agent") || blob.contains("mcp") || blob.contains("harness"))
        && SOFTWARE.is_match(blob)
    {
        score = 1;
    }
    score
}

fn lexical_engineering(video: &Signals<'_>, blob: &str) -> i32 {
    let mut score = 0;
    if has_category(video, "llms-generative-ai")
        || has_slug(video, ENGINEERING_SLUGS)
        || phrase(blob, ENGINEERING_STRONG)
    {
        score = 3;
    }
    if has_category(video, "machine-learning") && ML_PHRASE.is_match(blob) {
        score = score.max(2);
    }
    if score == 0 && (blob.contains("llm") || blob.contains("transformer") || blob.contains("gpt"))
    {
        score = 1;
    }
    score
}

fn lexical_research(video: &Signals<'_>, blob: &str) -> i32 {
    let mut score = 0;
    if has_category(video, "ai-safety-impact")
        || has_slug(video, RESEARCH_SLUGS)
        || phrase(blob, RESEARCH_STRONG)
    {
        score = 3;
    }
    if has_category(video, "machine-learning")
        && matches!(video.content_type, "lecture" | "talk" | "course")
        && ML_PHRASE.is_match(blob)
    {
        score = score.max(2);
    }
    if score == 0
        && (blob.contains("alignment") || blob.contains("safety") || blob.contains("arxiv"))
    {
        score = 1;
    }
    score
}

pub fn reason(why: &str, summary: &str, topic_slugs: &[String], topic: &str, area: &str) -> String {
    if let Some(sentence) = first_sentence(why) {
        return sentence;
    }
    if let Some(sentence) = first_sentence(summary) {
        return sentence;
    }
    if !topic_slugs.is_empty() {
        let shown = topic_slugs
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        return format!("AI Engineer topics: {shown}");
    }
    if !topic.is_empty() {
        return format!("Topic: {topic}");
    }
    format!("Matches {area}")
}

fn first_sentence(text: &str) -> Option<String> {
    let cleaned = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.is_empty() {
        return None;
    }
    for mark in [". ", "? ", "! "] {
        if let Some((head, _)) = cleaned.split_once(mark) {
            return Some(format!("{head}{}", mark.trim()));
        }
    }
    Some(cleaned)
}

pub fn cosine(left: &[f32], right: &[f32]) -> f32 {
    left.iter().zip(right).map(|(a, b)| a * b).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owned(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_string()).collect()
    }

    fn video<'a>(
        title: &'a str,
        summary: &'a str,
        topic: &'a str,
        tags: &'a str,
        slugs: &'a [String],
        categories: &'a [String],
        content_type: &'a str,
    ) -> Signals<'a> {
        Signals {
            title,
            summary,
            why: "",
            topic,
            tags,
            topic_slugs: slugs,
            categories,
            content_type,
            cosines: None,
        }
    }

    #[test]
    fn hide_and_seek_is_not_agentic_even_with_a_high_cosine() {
        let slugs = owned(&[]);
        let categories = owned(&["machine-learning"]);
        let mut signals = video(
            "OpenAI Plays Hide and Seek…and Breaks The Game!",
            "Two Minute Papers covers OpenAI's multi-agent hide-and-seek experiment, in which agents learned emergent tool use and found ways to break the game's rules.",
            "multi-agent reinforcement learning",
            "openai hide and seek emergent behavior reinforcement learning",
            &slugs,
            &categories,
            "explainer",
        );
        assert_eq!(classify(&signals).agentic, 0);
        signals.cosines = Some([0.9, 0.2, 0.2]);
        assert_eq!(classify(&signals).agentic, 0);
    }

    #[test]
    fn coding_agent_and_skills_are_agentic() {
        let slugs = owned(&[]);
        let categories = owned(&["llms-generative-ai"]);
        let vibe = video(
            "Vibe coding with Claude Code",
            "A coding agent edits a Rust repo with Claude Code.",
            "ai-assisted programming",
            "vibe coding claude code agentic ai",
            &slugs,
            &categories,
            "demo",
        );
        assert!(classify(&vibe).agentic >= 2);

        let talk_slugs = owned(&[
            "coding-agents",
            "model-context-protocol",
            "context-engineering",
        ]);
        let none = owned(&[]);
        let skills = video(
            "Don't Build Agents, Build Skills Instead",
            "Reusable skills package procedural knowledge for coding agents. MCP connects tools.",
            "agent skills",
            "coding agents mcp",
            &talk_slugs,
            &none,
            "talk",
        );
        assert!(classify(&skills).agentic >= 2);
    }

    #[test]
    fn stone_axe_and_comedy_are_out() {
        let slugs = owned(&[]);
        let categories = owned(&["home-diy-repair"]);
        let axe = video(
            "Primitive Technology: Stone Axe",
            "Tool use in the wild while making a stone axe.",
            "stone tools",
            "tool use",
            &slugs,
            &categories,
            "documentary",
        );
        assert_eq!(classify(&axe).agentic, 0);
        let cats = owned(&["llms-generative-ai"]);
        let joke = video(
            "Monty Python",
            "A comedy sketch about agents.",
            "comedy",
            "",
            &slugs,
            &cats,
            "comedy",
        );
        assert_eq!(classify(&joke).engineering, 0);
    }

    #[test]
    fn embedding_promotes_a_weak_on_topic_video_only() {
        let slugs = owned(&[]);
        let categories = owned(&["machine-learning"]);
        let mut weak = video(
            "Notes on agent harness design",
            "A short note about harnesses for software agents.",
            "agent harness",
            "harness",
            &slugs,
            &categories,
            "talk",
        );
        // "agent harness" is a strong phrase, so lexical already includes it.
        assert!(classify(&weak).agentic >= 2);
        let categories = owned(&[]);
        weak = video(
            "A talk about models",
            "We mention an agent once, without a software-engineering claim.",
            "models",
            "agent",
            &slugs,
            &categories,
            "talk",
        );
        assert_eq!(classify(&weak).agentic, 0);
        // No keyword signal: a high cosine must not invent one.
        weak.cosines = Some([0.9, 0.1, 0.1]);
        assert_eq!(classify(&weak).agentic, 0);
        let mut hinted = video(
            "A note on models",
            "An agent that edits the repository.",
            "models",
            "agent",
            &slugs,
            &categories,
            "talk",
        );
        assert_eq!(classify(&hinted).agentic, 1);
        hinted.cosines = Some([0.55, 0.1, 0.1]);
        assert!(classify(&hinted).agentic >= 2);
        hinted.cosines = Some([0.2, 0.1, 0.1]);
        assert_eq!(classify(&hinted).agentic, 1);

        let mut ants = video(
            "Coding Adventure: Ant and Slime Simulations",
            "An ant-inspired algorithm and slime mold behavior, using compute shaders.",
            "agent-based simulation",
            "slime mold ant simulation compute shaders",
            &slugs,
            &categories,
            "explainer",
        );
        ants.cosines = Some([0.6, 0.2, 0.1]);
        assert_eq!(classify(&ants).agentic, 0);
    }
}
