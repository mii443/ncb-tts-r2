"""Browser regression test; run with `uv run --with playwright==1.49.1 python tests/dashboard_browser.py`."""
from pathlib import Path
from playwright.sync_api import sync_playwright, expect

ASSETS = Path(__file__).resolve().parents[1] / 'src/transcription/web'
EPOCH = '0123456789abcdef0123456789abcdef'

with sync_playwright() as playwright:
    browser = playwright.chromium.launch(headless=True)
    page = browser.new_page(viewport={'width': 390, 'height': 844})
    errors = []
    page.on('pageerror', lambda error: errors.append(str(error)))
    page.add_init_script('''
        window.sources = [];
        window.accessStatus = 204;
        window.EventSource = class extends EventTarget {
            static OPEN = 1;
            constructor(url) { super(); this.url = url; this.readyState = 0; sources.push(this); }
            close() { this.readyState = 2; }
        };
        window.fetch = async () => ({ status: window.accessStatus });
        window.emit = (type, payload, id) => {
            const source = sources.at(-1);
            source.dispatchEvent(new MessageEvent(type, { data: JSON.stringify(payload), lastEventId: id || '' }));
        };
    ''')
    def serve(route):
        name = route.request.url.rsplit('/', 1)[-1]
        name = name if name in ('dashboard.js', 'dashboard.css') else 'dashboard.html'
        mime = {'dashboard.html': 'text/html', 'dashboard.js': 'text/javascript', 'dashboard.css': 'text/css'}[name]
        route.fulfill(body=(ASSETS / name).read_text(), content_type=mime)
    page.route('https://ncb.test/**', serve)
    page.goto('https://ncb.test/view/test')
    page.evaluate("sources.at(-1).readyState = 1; emit('open', {})")
    expect(page.locator('#status')).to_contain_text('ライブ')
    def transcript(seq, kind, text, lang='ja', stream=1, utterance='1-1'):
        page.evaluate("([event,id]) => emit('transcript', event, id)", [{
            'type': kind, 'text': text, 'lang': lang, 'stream_id': stream,
            'utterance_id': utterance, 'speaker': 'Alice' if stream == 1 else 'Bob',
        }, f'{EPOCH}:{seq}'])
    transcript(1, 'partial', '途中')
    expect(page.locator('.partial')).to_contain_text('途中')
    transcript(2, 'partial', '同時発話', stream=2)
    expect(page.locator('.partial')).to_have_count(2)
    transcript(3, 'translation', 'hello', 'en')  # Translation can arrive before final.
    transcript(4, 'final', '<script>window.injected=true</script>こんにちは')
    transcript(5, 'translation', '안녕하세요', 'ko')
    expect(page.locator('.card')).to_have_count(1)
    expect(page.locator('.card .translation')).to_have_count(2)
    expect(page.locator('.card .source')).to_contain_text('<script>')
    assert page.evaluate('window.injected === undefined')
    expect(page.locator('.partial')).to_have_count(1)
    transcript(5, 'translation', 'duplicate must be ignored', 'ko')
    expect(page.locator('[data-lang="ko"]')).to_have_text('ko 안녕하세요')
    assert page.evaluate('document.documentElement.scrollWidth <= innerWidth')
    page.set_viewport_size({'width': 1440, 'height': 900})
    assert page.locator('.shell').bounding_box()['width'] <= 720
    page.evaluate("document.dispatchEvent(new Event('visibilitychange'))")
    assert f'after={EPOCH}%3A5' in page.evaluate('sources.at(-1).url')
    page.evaluate("accessStatus=403; sources.at(-1).readyState=2; emit('error', {})")
    expect(page.locator('#status')).to_contain_text('VCへの再参加を待機中')
    page.evaluate('accessStatus=204')
    page.wait_for_function('sources.at(-1).readyState === 0', timeout=7000)
    page.evaluate("sources.at(-1).readyState=1; emit('open', {})")
    expect(page.locator('#status')).to_contain_text('ライブ')
    for seq in range(6, 110):
        transcript(seq, 'final', f'card {seq}', utterance=f'1-{seq}')
    expect(page.locator('.card')).to_have_count(100)
    page.evaluate("accessStatus=404; sources.at(-1).readyState=2; emit('error', {})")
    expect(page.locator('#status')).to_contain_text('通話終了')
    assert not errors, errors
    browser.close()
print('PASS: partial/final, simultaneous speakers, trilingual cards, XSS, replay deduplication, responsive layout, rejoin, card bounds, session end')
