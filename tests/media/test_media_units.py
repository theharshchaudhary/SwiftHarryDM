import pytest

from swiftdm.core.errors import ErrorCode, SdmError
from swiftdm.core.models import MediaOptions
from swiftdm.engines.media import formats
from swiftdm.engines.media.errormap import map_error
from swiftdm.engines.media.progress import Aggregator, is_postprocess, parse_file, parse_progress
from swiftdm.engines.media.validate import validate
from tests.media.conftest import needs_ffmpeg


@pytest.mark.parametrize("label,selector,audio", [
    ("best", "bv*+ba/b", False),
    ("1080", "bv*[height<=?1080]+ba/b[height<=?1080]", False),
    ("720", "bv*[height<=?720]+ba/b[height<=?720]", False),
    ("mp3", "ba/b", True),
    ("m4a", "ba[ext=m4a]/ba/b", True),
    ("garbage", "bv*+ba/b", False),
])
def test_presets(label, selector, audio):
    sel = formats.build(formats.options_for_label(label))
    assert sel.selector == selector and sel.audio_only is audio


def test_mp4_preset_prefers_h264_aac_and_falls_back():
    sel = formats.build(MediaOptions("mp4", max_height=1080))
    assert sel.selector.startswith("bv*[height<=?1080][vcodec^=avc1]+ba[acodec^=mp4a]/")
    assert sel.selector.endswith("/b[height<=?1080]") and "--merge-output-format" in sel.postprocess


def test_mp3_postprocessing_args():
    assert formats.build(MediaOptions("mp3")).postprocess == ("-x", "--audio-format", "mp3", "--audio-quality", "0")


def test_progress_parsing():
    p = parse_progress("SDMP 137|downloading|1024|2048|NA|512.5|2")
    assert p == {"format_id": "137", "status": "downloading", "downloaded": 1024.0, "total": 2048.0, "estimate": None,
                 "speed": 512.5, "eta": 2.0}
    assert parse_progress("SDMP broken") is None and parse_progress("[download] 5%") is None
    assert parse_file('SDMF "C:\\\\x\\\\a b.mp4"') == "C:\\x\\a b.mp4"
    assert is_postprocess("[Merger] Merging formats into") and is_postprocess("SDMQ started|Merger")


def test_aggregator_combines_video_and_audio():
    a = Aggregator({"137": 1000, "140": 200})
    a.feed(parse_progress("SDMP 137|downloading|500|1000|NA|100|5"))
    assert a.figures()[:2] == (500, 1200)
    a.feed(parse_progress("SDMP 137|finished|1000|1000|NA|NA|NA"))
    a.feed(parse_progress("SDMP 140|downloading|100|NA|210|100|1"))
    assert a.figures()[:2] == (1100, 1210)


@pytest.mark.parametrize("line,code,retry", [
    ("ERROR: [youtube] x: Sign in to confirm your age.", ErrorCode.AUTH_REQUIRED, False),
    ("ERROR: [youtube] x: Sign in to confirm you're not a bot.", ErrorCode.AUTH_REQUIRED, False),
    ("ERROR: [youtube] x: Private video. Sign in", ErrorCode.UNAVAILABLE, False),
    ("ERROR: [youtube] x: Video unavailable", ErrorCode.UNAVAILABLE, False),
    ("ERROR: x: The uploader has not made this video available in your country", ErrorCode.GEO_BLOCKED, False),
    ("ERROR: [x] y: This video is DRM protected", ErrorCode.UNSUPPORTED, False),
    ("ERROR: [youtube] x: Requested format is not available.", ErrorCode.FORMAT_GONE, False),
    ("ERROR: Unsupported URL: https://example.com/", ErrorCode.UNSUPPORTED, False),
    ("ERROR: unable to download video data: HTTP Error 403: Forbidden", ErrorCode.FORBIDDEN, True),
    ("ERROR: unable to download video data: HTTP Error 429: Too Many Requests", ErrorCode.RATE_LIMITED, True),
    ("ERROR: [Errno 28] No space left on device", ErrorCode.DISK_FULL, False),
    ("ERROR: Unable to download webpage: <urlopen error timed out>", ErrorCode.NETWORK, True),
    ("ERROR: something new and strange", ErrorCode.EXTRACTOR, False),
])
def test_error_map(line, code, retry):
    e = map_error(["[youtube] x: Downloading webpage", line], 1)
    assert e.info.code == code and e.info.retryable is retry


def test_error_map_redacts_signed_urls():
    e = map_error(["ERROR: something failed for https://cdn.example/v.mp4?sig=SECRET123"], 1)
    assert "SECRET123" not in e.info.message and "SECRET123" not in e.info.detail


@needs_ffmpeg
def test_validate_accepts_real_clip(clips):
    from swiftdm.engines.media.tools import find_ffprobe
    validate(find_ffprobe(), str(clips["av"]), audio_only=False, expect_audio=True, expect_height=360,
             max_height=1080, duration=6.0)


@needs_ffmpeg
@pytest.mark.parametrize("kw,needle", [
    (dict(audio_only=False, expect_audio=True, expect_height=None, max_height=None, duration=None), "no audio"),
    (dict(audio_only=False, expect_audio=False, expect_height=1080, max_height=None, duration=None), "expected 1080p"),
    (dict(audio_only=False, expect_audio=False, expect_height=None, max_height=200, duration=None), "above"),
    (dict(audio_only=False, expect_audio=False, expect_height=None, max_height=None, duration=60.0), "duration"),
    (dict(audio_only=True, expect_audio=True, expect_height=None, max_height=None, duration=None), "no audio"),
])
def test_validate_rejects_mismatches(clips, kw, needle):
    from swiftdm.engines.media.tools import find_ffprobe
    with pytest.raises(SdmError) as e:
        validate(find_ffprobe(), str(clips["video_only"]), **kw)
    assert needle in e.value.info.message


@needs_ffmpeg
def test_validate_rejects_non_media(tmp_path):
    from swiftdm.engines.media.tools import find_ffprobe
    junk = tmp_path / "junk.mp4"
    junk.write_bytes(b"<html>login page</html>")
    with pytest.raises(SdmError):
        validate(find_ffprobe(), str(junk), audio_only=False, expect_audio=False, expect_height=None,
                 max_height=None, duration=None)
