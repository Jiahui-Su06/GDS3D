from __future__ import annotations

import json
import os
from pathlib import Path
from typing import Any


DEFAULT_LOCALE = "en"
LOCALE_ENV_VAR = "GDS3D_LOCALE"
SUPPORTED_LOCALES = ("en", "zh-CN")

_LOCALE_DIR = Path(__file__).resolve().parent.parent / "i18n"
_catalog_cache: dict[str, dict[str, str]] = {}
_locale = os.environ.get(LOCALE_ENV_VAR, DEFAULT_LOCALE)


def set_locale(locale: str) -> None:
    global _locale
    if locale not in SUPPORTED_LOCALES:
        locale = DEFAULT_LOCALE
    _locale = locale


def locale() -> str:
    return _locale


def tr(key: str, **values: Any) -> str:
    return tr_for_locale(_locale, key, **values)


def tr_for_locale(locale_name: str, key: str, **values: Any) -> str:
    text = _lookup(locale_name, key)
    if values:
        return text.format(**values)
    return text


def _lookup(locale_name: str, key: str) -> str:
    catalog = _load_catalog(locale_name)
    text = catalog.get(key)
    if text is not None:
        return text

    fallback = _load_catalog(DEFAULT_LOCALE)
    return fallback.get(key, key)


def _load_catalog(locale_name: str) -> dict[str, str]:
    cached = _catalog_cache.get(locale_name)
    if cached is not None:
        return cached

    path = _LOCALE_DIR / f"{locale_name}.json"
    if not path.exists() and locale_name != DEFAULT_LOCALE:
        path = _LOCALE_DIR / f"{DEFAULT_LOCALE}.json"

    with path.open("r", encoding="utf-8") as file:
        raw = json.load(file)

    if not isinstance(raw, dict):
        raise ValueError(f"invalid i18n catalog: {path}")

    catalog: dict[str, str] = {}
    for key, value in raw.items():
        if not isinstance(key, str) or not isinstance(value, str):
            raise ValueError(f"invalid i18n entry in {path}")
        catalog[key] = value

    _catalog_cache[locale_name] = catalog
    return catalog
