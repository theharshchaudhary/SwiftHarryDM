import json

from swiftdm.config.settings import Settings, SettingsStore, resolve_engine
from swiftdm.logging_setup import redact, redact_url


def test_redact_url_strips_query_userinfo_fragment():
    assert redact_url("https://user:pw@cdn.example.com:8443/v/a.mp4?sig=SECRET&exp=1#t") == \
        "https://cdn.example.com:8443/v/a.mp4?<redacted>"


def test_redact_text_tokens_and_urls():
    text = 'GET https://x.example/a?token=abc Authorization: Bearer abc.def license_key="K3Y" token=zzz'
    out = redact(text)
    for secret in ("abc.def", "K3Y", "zzz", "token=abc"):
        assert secret not in out


def test_settings_roundtrip_and_validation(app_home):
    store = SettingsStore()
    assert store.get().engine in ("legacy", "v2")
    s = store.update(max_concurrent=99, max_segments=0, collision_policy="bogus", engine="v2")
    assert (s.max_concurrent, s.max_segments, s.collision_policy, s.engine) == (10, 1, "rename", "v2")
    assert SettingsStore().get().engine == "v2"


def test_settings_survive_corrupt_file(app_home):
    (app_home / "settings.json").write_text("{not json", encoding="utf-8")
    assert SettingsStore().get().max_concurrent == Settings().normalized().max_concurrent


def test_settings_ignore_unknown_keys(app_home):
    (app_home / "settings.json").write_text(json.dumps({"engine": "v2", "evil": 1}), encoding="utf-8")
    assert SettingsStore().get().engine == "v2"


def test_engine_resolution_precedence():
    s = Settings(engine="legacy")
    assert resolve_engine(["main.py"], {}, s) == "legacy"
    assert resolve_engine(["main.py"], {"SWIFTDM_ENGINE": "v2"}, s) == "v2"
    assert resolve_engine(["main.py", "--engine", "legacy"], {"SWIFTDM_ENGINE": "v2"}, s) == "legacy"
    assert resolve_engine(["main.py", "--engine=v2"], {}, s) == "v2"
    assert resolve_engine(["main.py", "--engine=bogus"], {}, s) == "legacy"
