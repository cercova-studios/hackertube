use assert_cmd::Command;
use serde_json::json;
use std::fs;
use std::path::Path;

fn run(home: &Path, args: &[&str]) -> assert_cmd::assert::Assert {
    Command::cargo_bin("hackertube")
        .unwrap()
        .env("HACKERTUBE_HOME", home)
        .env("HOME", home)
        .env("HACKERTUBE_TODAY", "2026-11-11")
        .env("HACKERTUBE_EMBED", "off")
        .env("HACKERTUBE_CAPTIONS", "off")
        .args(args)
        .assert()
}

fn write_dataset(root: &Path) {
    fs::create_dir_all(root).unwrap();
    let videos = [
        video("aaaaaaaaaaa", "Let's build GPT", "A spelled-out language model.", "HN's favorite GPT walkthrough.", "tutorial", "evergreen", "language models", &["gpt", "llm"], &["llms-generative-ai"], 10.0, 5),
        video("bbbbbbbbbbb", "AI safety alignment", "A lecture on AI safety and alignment.", "A clear alignment lecture.", "lecture", "evergreen", "ai safety", &["alignment"], &["ai-safety-impact"], 8.0, 4),
        video("ccccccccccc", "Vibe coding with Claude Code", "A coding agent edits a Rust repo.", "Shows a coding agent editing a repo.", "demo", "evergreen", "coding agents", &["claude code", "vibe coding"], &["software-practice-careers"], 3.0, 2),
        video("ddddddddddd", "Monty Python sketch", "A comedy sketch.", "Quoted for the joke.", "comedy", "evergreen", "comedy", &["sketch"], &["standup-sketch-comedy"], 20.0, 10),
        video("eeeeeeeeeee", "News model launch", "A one-day model announcement.", "Only matters this week.", "news", "news", "llm", &["llm"], &["llms-generative-ai"], 12.0, 3),
        video("fffffffffff", "Primitive Technology: Stone Axe", "Tool use in the wild.", "A practical demonstration.", "documentary", "evergreen", "stone tools", &["tool use"], &["home-diy-repair"], 6.0, 4),
        video("ggggggggggg", "Backpropagation lecture", "A neural network backpropagation lecture.", "The standard backprop derivation.", "lecture", "evergreen", "neural networks", &["backpropagation"], &["machine-learning"], 7.0, 3),
        video("hhhhhhhhhhh", "OpenAI Plays Hide and Seek", "Two Minute Papers covers OpenAI's multi-agent hide-and-seek experiment, in which agents learned emergent tool use.", "A game demo.", "explainer", "dated", "multi-agent reinforcement learning", &["hide and seek", "openai"], &["machine-learning"], 2.0, 4),
    ];
    let body = videos
        .iter()
        .map(|video| serde_json::to_string(video).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(root.join("videos.jsonl"), body).unwrap();
    fs::write(
        root.join("related.jsonl"),
        json!({"vid":"ddddddddddd","related":[{"vid":"aaaaaaaaaaa","shared_recommenders":2}]})
            .to_string()
            + "\n",
    )
    .unwrap();
}

#[allow(clippy::too_many_arguments)]
fn video(
    vid: &str,
    title: &str,
    summary: &str,
    why: &str,
    content_type: &str,
    shelf: &str,
    topic: &str,
    tags: &[&str],
    categories: &[&str],
    score: f64,
    recommenders: i64,
) -> serde_json::Value {
    json!({
        "vid": vid,
        "url": format!("https://www.youtube.com/watch?v={vid}"),
        "title": title,
        "channel": "Channel",
        "summary": summary,
        "why_hn": why,
        "content_type": content_type,
        "level": "intermediate",
        "shelf_life": shelf,
        "primary_topic": topic,
        "tags": tags,
        "categories": categories.iter().map(|id| json!({"id": id, "name": id, "area": "ai-data", "area_name": "AI"})).collect::<Vec<_>>(),
        "quote": {"text": why, "hn_comment_id": 1, "year": "2024"},
        "hn": {"recommenders": recommenders, "mentions": recommenders},
        "score": score
    })
}

fn write_talks(root: &Path) {
    fs::create_dir_all(root.join("markdown")).unwrap();
    let talk = include_str!("fixtures/talk.md");
    fs::write(root.join("markdown").join("abcdefghijk.md"), talk).unwrap();
    fs::write(root.join("talks.json"), serde_json::to_string(&json!([{
        "slug": "example-talk",
        "title": "Agents Need a Harness",
        "url": "https://ai.engineer/talks/abcdefghijk-example-talk",
        "videoId": "abcdefghijk",
        "event": "AI Engineer 2026",
        "speakers": [{"name": "Ada Lovelace"}],
        "topics": [
            {"slug": "coding-agents", "name": "Coding agents"},
            {"slug": "agent-engineering", "name": "Agent engineering"},
            {"slug": "context-engineering", "name": "Context engineering"}
        ],
        "summary": "Ada argues a coding agent should be graded on completed work. The talk covers MCP and context engineering."
    }])).unwrap()).unwrap();
    fs::write(
        root.join("transcripts.json"),
        serde_json::to_string(&json!([{
            "talkSlug": "example-talk",
            "title": "Agents Need a Harness",
            "jsonUrl": "https://ai.engineer/api/data/transcript?slug=example-talk&format=json",
            "csvUrl": "https://ai.engineer/api/data/transcript?slug=example-talk&format=csv"
        }]))
        .unwrap(),
    )
    .unwrap();
}

fn ingest(home: &Path) {
    let dataset = home.join("dataset");
    let talks = home.join("talks");
    write_dataset(&dataset);
    write_talks(&talks);
    run(
        home,
        &[
            "ingest",
            "--source",
            "hackertube",
            "--path",
            dataset.to_str().unwrap(),
        ],
    )
    .success();
    run(
        home,
        &[
            "ingest",
            "--source",
            "hackertube",
            "--path",
            dataset.to_str().unwrap(),
        ],
    )
    .success()
    .stdout(predicates::str::contains("false"));
    run(
        home,
        &[
            "ingest",
            "--source",
            "aietalks",
            "--path",
            talks.to_str().unwrap(),
        ],
    )
    .success();
}

#[test]
fn home_recommend_state_and_contract() {
    let home = std::env::temp_dir().join(format!("ht-cli-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(&home).unwrap();
    ingest(&home);
    run(&home, &[])
        .success()
        .stdout(predicates::str::contains("description:"))
        .stdout(predicates::str::contains("help["))
        .stdout(predicates::str::contains("picks["));
    let agentic = run(
        &home,
        &["recommend", "--area", "agentic-engineering", "--limit", "8"],
    )
    .success();
    let stdout = String::from_utf8(agentic.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("{vid,title,score,reason}:"), "{stdout}");
    assert!(stdout.contains("abcdefghijk"), "{stdout}");
    assert!(stdout.contains("ccccccccccc"), "{stdout}");
    assert!(!stdout.contains("hhhhhhhhhhh"), "{stdout}");
    assert!(!stdout.contains("fffffffffff"), "{stdout}");
    assert!(stdout.find("abcdefghijk").unwrap() < stdout.find("ccccccccccc").unwrap());

    let engineering = String::from_utf8(
        run(&home, &["recommend", "--area", "ai-engineering"])
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(engineering.contains("aaaaaaaaaaa"));
    assert!(!engineering.contains("eeeeeeeeeee"));
    let with_news = String::from_utf8(
        run(
            &home,
            &["recommend", "--area", "ai-engineering", "--include-news"],
        )
        .success()
        .get_output()
        .stdout
        .clone(),
    )
    .unwrap();
    assert!(with_news.contains("eeeeeeeeeee"));

    run(&home, &["seen", "aaaaaaaaaaa"])
        .success()
        .stdout(predicates::str::contains("changed: true"));
    run(&home, &["seen", "aaaaaaaaaaa"])
        .success()
        .stdout(predicates::str::contains("changed: false"));
    let ranked = String::from_utf8(
        run(&home, &["recommend", "--area", "ai-engineering"])
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(!ranked.contains("aaaaaaaaaaa"));
    assert!(ranked.contains("ggggggggggg"));
    run(&home, &["like", "ggggggggggg"]).success();
    let liked = String::from_utf8(
        run(&home, &["recommend", "--area", "ai-engineering"])
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(liked.contains("ggggggggggg"));
    run(&home, &["dislike", "ggggggggggg"]).success();
    let hidden = String::from_utf8(
        run(&home, &["recommend", "--area", "ai-engineering"])
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(hidden.contains("empty:"));
    run(&home, &["forget", "aaaaaaaaaaa"]).success();
    run(&home, &["forget", "ggggggggggg"]).success();
    let restored = String::from_utf8(
        run(&home, &["recommend", "--area", "ai-engineering"])
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(restored.contains("aaaaaaaaaaa"));
    run(&home, &["snooze", "aaaaaaaaaaa", "--until", "2026-11-11"]).success();
    let snoozed = String::from_utf8(
        run(&home, &["recommend", "--area", "ai-engineering"])
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(!snoozed.contains("aaaaaaaaaaa"));
    run(&home, &["snooze", "aaaaaaaaaaa", "--until", "2026-11-10"]).success();
    let expired = String::from_utf8(
        run(&home, &["recommend", "--area", "ai-engineering"])
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(expired.contains("aaaaaaaaaaa"));

    run(
        &home,
        &["search", "GPT walkthrough", "--fields", "vid,title"],
    )
    .success()
    .stdout(predicates::str::contains("{vid,title}:"))
    .stdout(predicates::str::contains("aaaaaaaaaaa"))
    .stdout(predicates::str::contains("semantic: off"));
    run(&home, &["search", "zzzz-not-a-topic"])
        .success()
        .stdout(predicates::str::contains("empty:"))
        .stdout(predicates::str::contains("videos[0]"));
    let json = String::from_utf8(
        run(&home, &["search", "GPT", "--json"])
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    let payload: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(payload["videos"].is_array());
    assert!(payload["help"].is_array());

    let insights = String::from_utf8(
        run(&home, &["insights", "abcdefghijk"])
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(insights.contains("method: deterministic"));
    assert!(insights.contains("github.com/browserbase/stagehand"));
    assert!(insights.contains("arxiv:2406.01234"));
    run(&home, &["transcript", "aaaaaaaaaaa"])
        .code(1)
        .stdout(predicates::str::contains("transcript_unavailable"));
    run(&home, &["insights", "abcdefghijk", "--llm"])
        .code(1)
        .stdout(predicates::str::contains("llm_unconfigured"));
    run(&home, &["--not-a-flag"])
        .code(2)
        .stdout(predicates::str::contains("exit: 2"));
    run(&home, &["recommend", "--area", "cooking"]).code(2);
    run(&home, &["show", "nope"]).code(2);
    run(&home, &["show", "zzzzzzzzzzz"])
        .code(1)
        .stdout(predicates::str::contains("not_found"));
    run(&home, &["recommend", "--help"])
        .success()
        .stdout(predicates::str::contains("focus area"));
    run(&home, &["setup"]).success();
    assert!(home.join("hooks").join("session.sh").exists());
    let settings: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(home.join(".claude").join("settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        settings["hooks"]["SessionStart"][0]["hooks"][0]["command"],
        "hackertube"
    );
    run(&home, &["setup"])
        .success()
        .stdout(predicates::str::contains("changed: false"));
    let _ = fs::remove_dir_all(&home);
}
