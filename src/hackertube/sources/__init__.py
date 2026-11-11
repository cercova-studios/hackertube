"""Pluggable catalog sources. Register a new source in SOURCES and add a fixture test."""

from __future__ import annotations

from hackertube.sources.aietalks import ingest_aietalks
from hackertube.sources.base import IngestContext, SourceResult
from hackertube.sources.hackertube_dataset import ingest_hackertube

SOURCES = {
    "hackertube": ingest_hackertube,
    "aietalks": ingest_aietalks,
}

__all__ = ["SOURCES", "IngestContext", "SourceResult", "ingest_aietalks", "ingest_hackertube"]
