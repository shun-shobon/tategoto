# Tategoto アプリ設計メモ

Tategoto は、macOS のマイク入力を常時文字起こしし、結果をローカルファイルへ保存する
個人用アプリである。現在の到達点は「メニューバー常駐アプリから録音を開始し、Realtime
Transcription API の completed event を日別 Markdown / JSONL に追記すること」とする。

この段階では、ChatGPT bearer token の期限切れ対応、再ログイン導線、組織向けの
プライバシー設計、議事録生成、検索、同期は扱わない。認証・接続・音声入力・ファイル
書き込みのいずれかが失敗した場合は `stopped_with_error` に遷移し、ユーザーが手元で
復旧してから再度開始する前提にする。

## 現状のゴール

- macOS のメニューバーに常駐し、録音状態を確認できる
- メインウィンドウから Start / Stop、入力デバイス、出力先を操作できる
- 入力デバイスをシステムデフォルト追従または特定デバイス固定から選べる
- `~/.codex/auth.json` の ChatGPT bearer token で Realtime transcription session を作る
- Rust 側でマイク入力を PCM16 mono 24 kHz に変換して WebSocket へ送る
- Realtime API の transcript completed event をローカルファイルへ追記する
- 日別の Markdown と JSONL を `~/Documents/Tategoto/` に残す

MVP では「録音・送信・保存の経路が単純で、失敗時に状態が分かること」を優先する。
リアルタイム表示、検索、編集、要約、復旧導線の作り込みは後回しにする。

## 非ゴール

- ChatGPT token の自動更新、再ログイン、認証状態監視
- 複数ユーザー、権限管理、共有、クラウド同期
- 保存期間、削除ポリシー、同意 UI などのプライバシー設計
- 話者分離、会議検出、議事録生成、要約
- transcript の全文検索、編集 UI、タグ付け
- HTTP file upload 経由の長尺音声一括文字起こし

## 基本構成

```text
Tauri app
  -> React UI
  -> Tauri tray / window commands
  -> Rust audio capture
  -> Rust Realtime transcription client
  -> Rust file writer
```

保存先を DB にしない。現段階ではローカルファイルを唯一の永続化先にして、実装と検証を
単純にする。将来、検索や編集が必要になった時点で DB や index を追加する。

## 技術スタック

- desktop shell: Tauri
- UI: React + TypeScript
- tray / menu bar integration: Tauri tray API
- audio capture: Rust + cpal
- transcription client: Rust + reqwest + tokio-tungstenite
- file output: Rust 側で Markdown / JSONL に追記
- frontend/backend bridge: Tauri command と event

React 側は状態表示と操作に寄せる。マイク入力、WebSocket、ファイル書き込みのような
長時間動作する処理は Rust 側に置く。これにより、UI の再描画や WebView の状態と、
録音・送信・保存のライフサイクルを分離する。

```text
React
  -> get_snapshot command
  -> start_transcription / stop_transcription command
  -> refresh_input_devices command
  -> update_settings command
  -> open_today_markdown / open_output_directory command
  <- transcription_state_changed event
  <- transcript_segment_written event
  <- transcription_error event

Rust
  -> input device resolver
  -> audio stream
  -> realtime session manager
  -> server event timeline
  -> transcript file writer
```

Tauri を使うが、音声 capture を WebView の `getUserMedia` には寄せない。入力デバイスの
列挙、固定、resample、停止処理を Rust 側で扱う。

## 認証

`~/.codex/auth.json` にある `tokens.access_token` を読み取り、
`POST https://api.openai.com/v1/realtime/transcription_sessions` で transcription
session を作る。

session 作成、WebSocket 接続、または transcription 中に認証エラーが返った場合は
`stopped_with_error` に遷移し、最後のエラーを UI に表示する。自動再認証や refresh は
実装しない。

## 入力デバイス

入力デバイスは固定で内蔵マイクにしない。次の 2 モードを設定できる。

- `system_default`: macOS のデフォルト入力デバイスを使う
- `fixed_device`: ユーザーが選んだ入力デバイスを device id で固定する

初期値は `system_default` にする。設定は Tauri の app config directory に
`settings.json` として保存する。

保存する情報は次の通り。

```json
{
  "input_device_mode": "fixed_device",
  "input_device_id": "72a2...",
  "input_device_name": "MacBook Pro Microphone",
  "transcription": {
    "model": "gpt-4o-transcribe",
    "language": null,
    "prompt": null,
    "noise_reduction": null,
    "turn_detection": {
      "threshold": 0.5,
      "prefix_padding_ms": 300,
      "silence_duration_ms": 700
    }
  }
}
```

`input_device_name` は表示用であり、録音に使う識別子は `input_device_id` とする。
現在の `input_device_id` は cpal が返す入力デバイスの列挙順と名前から作る hash である。
OS から永続的な hardware id を取得しているわけではないため、同名デバイスや列挙順変更に
完全に強い識別子ではない。

`system_default` の場合は、録音開始時にその時点のデフォルト入力デバイスを解決する。
録音中に macOS のデフォルト入力が変わった場合の自動追従はしない。

`fixed_device` の場合は、録音開始時に保存済み device id を探す。見つからない場合は
勝手にデフォルトへ落とさず、`stopped_with_error` として停止する。固定指定は「この
デバイスで録る」という意味なので、暗黙の fallback は入れない。

## 文字起こし設定

Realtime transcription session 作成時に使う設定は UI から変更できる。変更は次回の
録音開始時に反映する。録音中と session 張り替え中は、入力デバイスを含む設定 UI を
無効化する。

設定できる項目は次の通り。

- 認識モデル: `gpt-4o-transcribe` または `gpt-4o-mini-transcribe`
- 音声の言語: 未指定または Realtime Transcription API が受け付ける ISO-639-1 code
- 認識ヒント: 固有名詞、専門用語、文脈などを渡す prompt
- ノイズ低減: 未指定、`near_field`、`far_field`
- 発話区切り: `server_vad` の `threshold`、`prefix_padding_ms`、
  `silence_duration_ms`

初期値は高精度モデル、言語未指定、prompt なし、ノイズ低減なし、`server_vad` の
`threshold: 0.5`、`prefix_padding_ms: 300`、`silence_duration_ms: 700` とする。
文字起こし設定のリセットはこの初期値だけを復元し、入力デバイス設定は変更しない。

## 音声入力

Rust 側で cpal の入力 stream を作り、デバイスの default input config に従って受け取った
サンプルを PCM16 mono 24 kHz に変換する。対応している input sample format は `f32`、
`i16`、`u16` である。

音声は cpal callback ごとの block として Realtime WebSocket に
`input_audio_buffer.append` で送る。クライアント側では固定長 chunk を組み立てず、通常時は
session 作成時に指定した server VAD に commit を任せる。

停止時と session 張り替え時だけ、未確定の audio buffer が 100ms 以上ある場合に
`input_audio_buffer.commit` を明示的に送る。commit 後は completed event を最大 10 秒待ち、
保存できた transcript を Markdown / JSONL に追記してから session を閉じる。

transcript の時間情報は、audio block の capture 時刻と Realtime event の audio offset から
組み立てる。offset が取れない場合は現在時刻で補う。

マイク権限がない場合や input stream を開始できない場合は録音開始に失敗する。現状では
権限設定画面への誘導は行わず、エラーとして表示する。

## Realtime session 管理

Realtime transcription session は 50 分ごとに張り替える。

現在の張り替え処理は次の通り。

1. 状態を `rotating_session` にする
2. 旧 session へ送信待ちの audio block を drain する
3. 旧 session に `input_audio_buffer.commit` を送り、completed event を最大 10 秒待つ
4. flush 中に届いた新しい audio block を一時退避する
5. 旧 session の WebSocket を閉じる
6. 新しい transcription session を作る
7. timeline を初期化する
8. 退避した audio block を新 session へ送る
9. 状態を `recording` に戻す

flush が timeout または Realtime API error になった場合は録音をエラー停止しない。
未保存の発話がある可能性を warning として UI に残し、停止処理または session 張り替えを続ける。
空または短すぎる buffer の commit rejection は、保存対象がないものとして warning にしない。

## transcript 出力

主出力は Markdown と JSONL の 2 種類にする。

```text
~/Documents/Tategoto/
  2026-05-05.md
  2026-05-05.jsonl
```

Documents directory が取れない場合は home directory 配下の `Tategoto` を使う。

Markdown は人間が読むための出力、JSONL は後から再処理するための構造化ログとして使う。

Markdown の例:

```md
# 2026-05-05

## 09:15
- [09:15:02-09:15:18] 今日の作業はまず設計書の見直しから始めます。
```

JSONL の例:

```jsonl
{"type":"transcript_segment","local_start":"2026-05-05T09:15:02+09:00","local_end":"2026-05-05T09:15:18+09:00","session_id":"sess_...","item_id":"item_...","previous_item_id":null,"model":"gpt-4o-transcribe","text":"今日の作業はまず設計書の見直しから始めます。","received_at":"2026-05-05T09:15:19+09:00"}
```

保存時に持つ情報は次の通り。

- local start/end timestamp
- session id
- item id
- previous item id
- model name
- transcript text
- completion received timestamp

Markdown は text が空でない segment だけ追記する。JSONL は空 text の segment も構造化ログ
として追記する。

local start/end timestamp は Realtime API の audio offset を audio capture 時刻に足して作る。
該当 event から offset を取得できない場合は現在時刻で補う。

## UI

主 UI はメニューバー常駐にする。Dock に常時出る通常ウィンドウアプリより、録音状態を
小さく確認でき、開始・停止しやすい方が用途に合う。

アプリは `ActivationPolicy::Accessory` で起動し、メインウィンドウの close は終了ではなく
hide として扱う。Quit は tray menu から実行する。

メニューバーには状態が分かる title / tooltip を出す。

- `idle`: `○ 待機中`
- `recording`: `● 録音中`
- `rotating_session`: `◐ 更新中`
- `stopped_with_error`: `! エラー`

tray menu に置く操作は次の通り。

- Show
- Start
- Stop
- Quit

入力デバイス選択、出力ファイルを開く操作、最後のエラー表示は React のメインウィンドウに
置く。メニューバーのポップオーバーや tray menu にはまだ置かない。

React の画面の責務は次に限定する。

- 現在状態の表示
- Start / Stop の発火
- 入力デバイス一覧の表示と再取得
- `system_default` / `fixed_device` の選択
- 今日の Markdown / JSONL path と出力 directory の表示
- 今日の Markdown と出力 directory を開く
- 最後のエラーの表示
- 最後の警告の表示

録音中の状態遷移は Rust 側を正とし、React 側は Tauri event と command response で表示を
更新する。

## アプリの状態

UI または tray で確認できる状態は次の 4 つとする。

- `idle`: 文字起こししていない
- `recording`: マイク入力を取り、transcription session に送っている
- `rotating_session`: 50 分ごとの session 張り替え中
- `stopped_with_error`: 認証、接続、音声入力、ファイル書き込みなどで停止した

`stopped_with_error` からの自動復旧はしない。原因を表示し、ユーザーが再度 Start する。
flush timeout や flush 中の Realtime API error は `stopped_with_error` にせず、`last_warning`
として表示する。

## 現状の未解消点

現状コードと目標仕様の主な差分は次の通り。

- 固定長 chunk builder はなく、server VAD による commit に依存している
- 入力デバイス id は cpal の列挙順と名前から作る hash で、OS の永続 id ではない
- tray menu は Start / Stop / Show / Quit のみで、入力デバイスや出力先操作は React window 側にある
- transcript のアプリ内 preview はない
- マイク権限エラー時の設定画面誘導はない

## 検証項目

- `system_default` で録音開始時のデフォルト入力デバイスが使われる
- `fixed_device` で選択した入力デバイスが使われる
- 固定デバイスが外れている場合、デフォルトに fallback せずエラー停止する
- メニューバーから Show / Start / Stop / Quit ができる
- React window から Start / Stop、出力ファイル確認、入力デバイス選択ができる
- 5 分程度の手元発話が Markdown と JSONL に保存される
- 無音を含む状態でもアプリが落ちずに動き続ける
- 停止操作時に最後の未確定発話が保存される
- session 張り替え境界でも旧 session の最後と新 session の最初が保存される
- flush が 10 秒以内に完了しない場合、エラー停止せず警告が表示される
- 60 分を超える運用で session 張り替え後も出力が続く
- ファイル書き込みに失敗した場合、エラー状態で停止する

## 後で考えること

MVP が動いた後に、必要に応じて次を追加する。

- token 期限切れ時の再接続、再ログイン、ユーザー通知
- ローカル VAD、overlap、重複除去による transcript 品質改善
- transcript の DB 化、検索、編集 UI
- transcript の直近 preview
- 5〜15 分ごとの小要約と議事録生成
- 保存期間、削除、録音中表示、送信制御などのプライバシー設計
- 録音中の入力デバイス変更の自動追従
- 話者分離モデルまたは別経路による speaker attribution
