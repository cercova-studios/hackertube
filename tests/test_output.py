from hackertube.output import Table, render, truncate


def test_table_declares_width_and_quotes_commas() -> None:
    table = Table(("vid", "title"), [{"vid": "abc", "title": "Hello, world"}])
    text = render({"videos": table, "help": ["hackertube show abc"]}, as_json=False)
    assert "videos[1]{vid,title}:" in text
    assert '"Hello, world"' in text
    assert "help[1]: hackertube show abc" in text


def test_empty_table_keeps_fields() -> None:
    text = render({"videos": Table(("vid", "title", "score", "reason"), [])}, as_json=False)
    assert "videos[0]{vid,title,score,reason}:" in text


def test_json_round_trip_shape() -> None:
    import json

    table = Table(("vid", "score"), [{"vid": "abc", "score": 10.0}])
    payload = json.loads(render({"ok": True, "videos": table}, as_json=True))
    assert payload["videos"] == [{"vid": "abc", "score": 10}]


def test_truncate_reports_omitted_chars() -> None:
    text = truncate("abcdefghij", 8)
    assert text.endswith("c]")
    assert "…[+" in text
