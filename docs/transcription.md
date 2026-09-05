# 文字起こし・翻訳・Web UI

`rstt` の `bbcfad7` にある文字起こし・翻訳・Web UI を本Botへ統合しています。
TTSと文字起こしは同じBot・Songbird音声接続を使います。推論サーバーは従来どおり
hayamimiのmultiplex `/ingest/v2` です。翻訳にはhayamimiの `--translate` が必要です。

## 機能の切り替え

通常の `cargo build --release --locked` は全機能が有効です。実行時も、ビルドに含まれる
機能は既定で有効です。有効な機能の必須設定がない場合は起動時にエラーを表示します。

| 構成 | ビルド | 実行時 |
|---|---|---|
| TTS・文字起こし・翻訳・Web UI | 通常のビルド | 通常の起動 |
| TTS・文字起こし・翻訳 | `--no-default-features --features transcription` | `NCB_WEB_ENABLED=false` |
| TTSのみ | `--no-default-features` | `NCB_TRANSCRIPTION_ENABLED=false` |

`web-ui` featureは `transcription` に依存します。文字起こしを実行時に無効化するとWeb UIも
無効になります。無効な機能の接続・受信デコード・HTTP待受・コマンド登録は行いません。
Web UIのみを無効化した場合は `/transcribe web` を登録せず、OAuth設定も不要です。
文字起こしが無効ならhayamimi設定も不要です。ビルドから除外した機能は実行時には有効化できません。

Dockerでも同じ構成を選べます。

```sh
docker build -t ncb-tts-r2 .
docker build --build-arg CARGO_FEATURES=transcription -t ncb-tts-r2:transcription .
docker build --build-arg CARGO_FEATURES= -t ncb-tts-r2:tts-only .
```

## 設定

既存のNCB_TOKEN、NCB_APP_ID、Redis、GCP/VOICEVOX設定に、次を追加します。
追加機能の環境変数は `config.toml` の同じ設定を上書きします。

| 環境変数 | TOML | 既定値・用途 |
|---|---|---|
| `NCB_TRANSCRIPTION_ENABLED` | `transcription.enabled` | コンパイル済みなら `true` |
| `HAYAMIMI_URL` | `transcription.url` | `ws://127.0.0.1:8766/ingest/v2` |
| `HAYAMIMI_BRIDGE_SECRET` | `transcription.secret` | 文字起こし有効時に必須 |
| `NCB_REQUIRE_CONSENT` | `transcription.require_consent` | `false` |
| `NCB_WEB_ENABLED` | `web.enabled` | コンパイル済みなら `true` |
| `NCB_WEB_BASE_URL` | `web.base_url` | Web有効時に必須。例 `https://rstt.mii.dev` |
| `NCB_WEB_BIND` | `web.bind` | `127.0.0.1:8080`。コンテナでは `0.0.0.0:8080` |
| `NCB_WEB_CLIENT_ID` | `web.client_id` | `NCB_APP_ID` と同じID |
| `NCB_WEB_CLIENT_SECRET` | `web.client_secret` | Web有効時に必須 |

真偽値の環境変数は `true/false`, `1/0`, `yes/no`, `on/off` に対応します。
旧名 `RSTT_REQUIRE_CONSENT`, `RSTT_WEB_BASE_URL`, `RSTT_WEB_BIND`, `DISCORD_CLIENT_ID`,
`DISCORD_CLIENT_SECRET` も使えます。新名と旧名が両方あれば新名が優先します。

TOMLは既存設定に次のテーブルを追加します。secretは環境変数から渡してください。

```toml
[transcription]
enabled = true
url = "ws://hayamimi:8766/ingest/v2"
require_consent = false

[web]
enabled = true
bind = "0.0.0.0:8080"
base_url = "https://rstt.mii.dev"
```

OAuthアプリには `<公開URL>/auth/discord/callback` をRedirect URIとして登録します。
OAuth scopeは `identify` だけで、既存rsttのOAuthアプリを継続利用する場合はそのClient ID/Secretを
明示してください。通話参加者の認可はNCB Botの現在のVoice Stateで行います。OAuth認証が
返すDiscord User IDはアプリをまたいで同じなので、OAuthアプリとBotのApplication IDは
異なっていても構いません。Cookieの署名形式・名前は引き継いでいます。
同じ公開ホストとOAuth secretなら7日間のログイン状態も維持されます。

## Discord操作と接続の共有

- `/transcribe start`: 実行者がいるVCで開始。Administrator / Manage Server / Move Membersの
  いずれかが必要です。開始通知とWebリンクをテキストチャンネルへ送ります。
- `/transcribe stop`: 文字起こしを停止。同じ権限が必要です。TTSが動作中ならVCに残ります。
- `/transcribe status`: 認識セッション、hayamimi接続、ストリーム数、同意方式を表示。
- `/transcribe web`: 対象VCの参加者へWebリンクを再表示。
- `/transcribe consent` / `revoke`: 本人の送信を有効化／停止。
- `/setup` / `/stop` / `/skip` / `/config`: 従来のTTS操作です。`/stop` しても文字起こしは継続します。

同じGuildで両機能を使う場合は同じVCを使ってください。別VCへの開始・自動参加は拒否します。
Botが退出／移動した場合は元の文字起こしを無効化し、異なるVCの音声が旧リンクへ流れることを防ぎます。
空のVCは定期監視で終了し、接続障害は既存の接続監視で再試行します。
Bot再起動時は認識セッションとWebリンクは失効します。新しく `/transcribe start` してください。

## 移植した処理と検証範囲

| rsttの機能 | 移植先・検証 |
|---|---|
| DAVE音声受信、SSRCとDiscord IDの対応、同時発話分離 | `transcription/bot.rs`, `router.rs`。NCB既存Songbirdのreceiveを有効化 |
| 16 kHz mono PCM s16le、18-byte v2ヘッダー | `protocol.rs` のhayamimi共通固定フレーム契約テスト |
| 未解決SSRCバッファ、末尾無音、idle、gap、bounded audio queue、再接続 | `router.rs`, `bridge.rs` とWebSocket通信テスト |
| opt-out / 明示同意 / 撤回 / 停止後のfinal・翻訳の短時間配信 | routerの同意・セッション分離・drainテスト |
| OAuth state、HttpOnly署名Cookie、VC在籍確認、セッション間分離 | `web.rs` の認証テストとHTTP経由のアクセス・SSE失効テスト |
| partial/final、日英韓翻訳、表示名・アバター | 移植したHTML/CSS/JSとHTTP/SSE翻訳テスト。内容はDiscordに投稿しません |
| SSEイベントID、容量・時間制限付きreplay、退出後の参加待ち、画面復帰時の再接続 | `web.rs`, `web/dashboard.js`。replay・期限・再入室テスト |
| 外部JS/CSS、CSP、no-cache再検証、SSE proxy buffering無効 | HTTPレスポンステスト。Gatewayでも `/api/events/` のtimeoutを無効にします |

全機能、文字起こしのみ、TTSのみの3構成をCIでテストします。Redis統合テストは一時Unix socketの
専用Redisを使います。実利用者のDiscord音声・OAuth操作はユニットテストでは実行しません。
