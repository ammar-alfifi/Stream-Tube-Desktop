# A1/A5 — network troubleshooting trail (real commands, real outputs)

Context: the first harness runs proved A1 (ANDROID_VR extraction + 206 Range GETs
on googlevideo), but the A5 public fallback failed from this host in two ways.
Both were isolated with curl before any code change; raw outputs below are
copied verbatim from the sessions.

## Symptom 1 — `https://media.w3.org/2010/05/sintel/trailer.mp4` answered 403 in the harness

The harness sent the desktop `BROWSER_UA` (Chrome 140) to every host. Isolation:

```
$ UA="Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36"
$ curl -s -o /dev/null -w '%{http_code} %{size_download}B\n' -H "User-Agent: $UA" -H 'Range: bytes=0-1048575' --max-time 25 https://media.w3.org/2010/05/sintel/trailer.mp4
403 5747B
$ curl -s -o /dev/null -w '%{http_code} %{size_download}B\n' -H 'Range: bytes=0-1048575' --max-time 25 https://media.w3.org/2010/05/sintel/trailer.mp4
206 1048576B
$ curl -s -o /dev/null -w '%{http_code} %{size_download}B\n' -H "User-Agent: $UA" --max-time 25 https://media.w3.org/2010/05/sintel/trailer.mp4
403 5726B
```

Root cause: the host's edge (Cloudflare, 104.18.22.19/23.19) answers **403 to a
browser User-Agent** and **206 to a plain client UA**, with or without Range.
Not a YouTube-side block. Fix in the harness: plain UA for non-YouTube hosts,
`BROWSER_UA` only for youtube.com/googlevideo.com.

## Symptom 2 — mux test HLS: ureq transport stalls (timeouts) while curl succeeds

```
$ for i in 1 2; do curl -s -o /dev/null -w '%{http_code} %{size_download}B\n' --max-time 25 https://test-streams.mux.dev/x36xhzz/url_0/193039199_mp4_h264_aac_hd_7.m3u8; done
200 3606B
200 3606B
$ for i in 1 2 3; do curl -s -o /dev/null -w 'mux variant + BROWSER_UA -> %{http_code} %{size_download}B\n' -H "User-Agent: $UA" --max-time 25 https://test-streams.mux.dev/x36xhzz/url_0/193039199_mp4_h264_aac_hd_7.m3u8; done
mux variant + BROWSER_UA -> 200 3606B
mux variant + BROWSER_UA -> 200 3606B
mux variant + BROWSER_UA -> 200 3606B
```

curl: 7/7 successes on the same URLs (any UA). ureq 3.5.2/rustls in the harness:
stalled on the A5 fetch in both early runs (one run had no timeout configured and
hung — fixed by adding the same 30 s budget as `http::agent`; the second run
timed out on the variant playlist after the master had already returned 200).
No AAAA records exist for either host, so it is not an IPv6 blackhole:

```
$ getent ahosts test-streams.mux.dev | sort -u | head
199.232.82.132  DGRAM
199.232.82.132  RAW
199.232.82.132  STREAM j.sni.global.fastly.net
```

Conclusion: an intermittent transport-level stall on this network's ureq/rustls
path to Fastly, per-connection (the master playlist did succeed over ureq in the
second run). Fix in the harness: each GET now retries transport errors up to 3
attempts. If retries are exhausted, the run records the failures verbatim and
the verdict reports `fallback_ok=false`.
