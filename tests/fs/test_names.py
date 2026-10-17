import pytest

from swiftdm.fs.names import (MAX_NAME_UNITS, choose_filename, parse_content_disposition, sanitize_component,
                              utf16_units)

# Subset of the tc2231 / RFC 6266 test cases plus traversal attempts.
CD_VECTORS = [
    ('attachment; filename="foo.html"', "foo.html"),
    ('inline; filename="foo.pdf"', "foo.pdf"),
    ("attachment; filename=foo.html", "foo.html"),
    (r'attachment; filename="f\oo.html"', "foo.html"),
    (r'attachment; filename="\"quoting\" tested.html"', '"quoting" tested.html'),
    ('attachment; filename="Here\'s a semicolon;.html"', "Here's a semicolon;.html"),
    ('attachment; foo="bar"; filename="foo.html"', "foo.html"),
    ('attachment; FILENAME="foo.html"', "foo.html"),
    ("attachment; filename*=UTF-8''foo-%c3%a4-%e2%82%ac.html", "foo-ä-€.html"),
    ("attachment; filename*=iso-8859-1''foo-%E4.html", "foo-ä.html"),
    ("attachment; filename=\"foo-ae.html\"; filename*=UTF-8''foo-%c3%a4.html", "foo-ä.html"),
    ("attachment; filename*=UTF-8''foo-%c3%a4.html; filename=\"foo-ae.html\"", "foo-ä.html"),
    ("attachment; filename*=bogus''x.html; filename=\"fallback.html\"", "fallback.html"),
    ('attachment; filename="fooÃ¤.html"', "fooä.html"),                 # raw UTF-8 seen as latin-1
    ('attachment; filename="/foo.html"', "foo.html"),
    ('attachment; filename="../../evil.exe"', "evil.exe"),
    (r'attachment; filename="..\\..\\evil.exe"', "evil.exe"),
    ("attachment; filename*=UTF-8''..%2F..%2Fevil.exe", "evil.exe"),
    ('attachment; filename="."', None),
    ('attachment; filename=""', None),
    ("attachment", None),
    ("", None),
]


@pytest.mark.parametrize("header,expected", CD_VECTORS)
def test_content_disposition(header, expected):
    assert parse_content_disposition(header) == expected


SANITIZE = [
    ("CON", "_CON"), ("con.txt", "_con.txt"), ("LPT1.tar.gz", "_LPT1.tar.gz"), ("COM¹.txt", "_COM¹.txt"),
    ("nul .txt", "_nul .txt"), ("CONSOLE.txt", "CONSOLE.txt"),
    ('a<b>c:d"e/f\\g|h?i*j.txt', "a_b_c_d_e_f_g_h_i_j.txt"),
    ("name. . ", "name"), ("  spaced.txt ", "spaced.txt"), ("", "download"), ("...", "download"),
    ("tab\there\x01.bin", "tab_here_.bin"), ("é.txt", "é.txt"),
]


@pytest.mark.parametrize("raw,expected", SANITIZE)
def test_sanitize(raw, expected):
    assert sanitize_component(raw) == expected


@pytest.mark.parametrize("raw", ["é" * 300 + ".mp4", "😀" * 200 + ".txt", "x" * 1000, "a." + "b" * 9 + "c" * 400])
def test_sanitize_length_limits(raw):
    out = sanitize_component(raw)
    assert 0 < utf16_units(out) <= MAX_NAME_UNITS
    out.encode("utf-16-le")   # no broken surrogates
    if raw.endswith((".mp4", ".txt")):
        assert out.endswith(raw[-4:])


@pytest.mark.parametrize("override,cd,url,ctype,expected", [
    (None, None, "http://h/file.bin", "application/octet-stream", "file.bin"),
    (None, None, "http://h/download?id=1", "application/zip", "download.zip"),
    (None, None, "http://h/", "text/plain", "download.txt"),
    (None, None, "http://h/", None, "download"),
    (None, 'attachment; filename="report.pdf"', "http://h/get?id=9", "application/pdf", "report.pdf"),
    ("my name.iso", 'attachment; filename="x.bin"', "http://h/y", None, "my name.iso"),
    (None, None, "http://h/my%20file%20%E2%82%AC.pdf", None, "my file €.pdf"),
    (None, None, "http://h/a/CON", "application/x-msdownload", "_CON.exe"),
])
def test_choose_filename(override, cd, url, ctype, expected):
    assert choose_filename(override, cd, url, ctype) == expected
