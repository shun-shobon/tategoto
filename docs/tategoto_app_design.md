# Tategoto MVP 設計メモ

Tategoto は、Mac のマイク入力を常時文字起こしし、結果をローカルファイルへ保存する
個人用アプリとしてまず成立させる。最初の到達点は「起動している間の発話が、時刻付き
の transcript としてファイルに残ること」とする。

この段階では、ChatGPT bearer token の期限切れ対応、再ログイン導線、組織向けの
プライバシー設計、議事録生成、検索、同期は扱わない。認証が失敗した場合はエラーを
表示して停止し、ユーザーが手元で復旧する前提にする。

## MVP のゴール

- Mac のマイク入力をアプリ起動中に継続取得する
- 入力デバイスをシステムデフォルト追従または特定デバイス固定から選べる
- ChatGPT bearer token で Realtime transcription session を作る
- 音声を短い単位で API に送り、文字起こし結果を受け取る
- 受け取った transcript をローカルファイルへ追記する
- メニューバー常駐 UI から録音状態と出力先を確認できる
- アプリ停止後も、その日の transcript をテキストとして読める

MVP では「欠落しにくく保存されること」を優先し、リアルタイム表示の作り込みや
高度な後処理は後回しにする。

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

保存先を最初から DB にしない。MVP ではローカルファイルを唯一の永続化先にして、
実装と検証を単純にする。将来、検索や編集が必要になった時点で DB や index を追加する。

## 技術スタック

MVP は Tauri + React で実装する。

- desktop shell: Tauri
- UI: React + TypeScript
- tray / menu bar integration: Tauri tray API
- audio capture: Rust 側で実装する
- transcription client: Rust 側で Realtime API に接続する
- file output: Rust 側で Markdown / JSONL に追記する
- frontend/backend bridge: Tauri command と event

React 側は状態表示と操作に寄せる。マイク入力、WebSocket、ファイル書き込みのような
長時間動作する処理は Rust 側に置く。これにより、UI の再描画や WebView の状態と、
録音・送信・保存のライフサイクルを分離する。

```text
React
  -> start_transcription / stop_transcription command
  -> list_input_devices command
  -> update_settings command
  <- transcription_state_changed event
  <- transcript_segment_written event
  <- transcription_error event

Rust
  -> input device resolver
  -> audio stream
  -> chunk builder
  -> realtime session manager
  -> transcript normalizer
  -> file writer
```

Tauri を使うが、音声 capture を WebView の `getUserMedia` には寄せない。入力デバイスの
列挙、固定、長時間録音、resample、停止時 flush を Rust 側で一貫して扱う。

## 認証

`~/.codex/auth.json` にある ChatGPT bearer token を読み取り、
`POST https://api.openai.com/v1/realtime/transcription_sessions` で transcription
session を作る。

この設計段階では token の期限切れを復旧対象にしない。session 作成、WebSocket 接続、
または transcription 中に認証エラーが返った場合は、現在の buffer を破棄せず処理を
止め、エラーをログに残す。自動再認証や refresh は実装しない。

## 入力デバイス

入力デバイスは固定で内蔵マイクにしない。MVP でも、次の 2 モードを設定できるようにする。

- `system_default`: macOS のデフォルト入力デバイスを使う
- `fixed_device`: ユーザーが選んだ入力デバイスを device id で固定する

初期値は `system_default` にする。外部マイクやオーディオインターフェイスを常用する場合は
`fixed_device` を選ぶ。

設定として保存する情報は次の通り。

```json
{
  "input_device_mode": "fixed_device",
  "input_device_id": "BuiltInMicrophoneDevice",
  "input_device_name": "MacBook Pro Microphone"
}
```

`input_device_name` は表示用であり、録音に使う識別子は `input_device_id` とする。
同名デバイスが複数ある場合や OS 側の表示名が変わる場合に備え、名前だけで解決しない。

`system_default` の場合は、録音開始時にその時点のデフォルト入力デバイスを解決する。
録音中に macOS のデフォルト入力が変わった場合の自動追従は MVP では必須にしない。
必要になったら、デバイス変更通知を受けて capture stream を張り替える。

`fixed_device` の場合は、録音開始時に保存済み device id を探す。見つからない場合は
勝手にデフォルトへ落とさず、`stopped_with_error` として停止する。固定指定は「この
デバイスで録る」という意味なので、暗黙の fallback は入れない。

## 音声入力

Mac 側でマイク入力を PCM16 mono 24 kHz に揃えて扱う。アプリ内部では、API へ送る
前の音声を短い buffer として持つ。

最初は固定長 chunk でよい。例えば 10〜30 秒ごとに commit し、API から返る
completed event を保存する。発話の途中で切れる品質問題が目立つ場合にだけ、ローカル
VAD や overlap を追加する。

MVP で欲しい挙動は次の通り。

- 録音開始から停止まで、音声 chunk を連続して送る
- chunk ごとに local start/end timestamp を付ける
- API 送信前の未 commit buffer はメモリ上に保持する
- 停止操作時は現在の buffer を commit してから終了する

マイク権限がない場合は録音開始に失敗する。MVP では権限要求と失敗表示だけを行い、
権限設定画面への丁寧な誘導は後回しにする。

## Realtime session 管理

Realtime transcription session は 1 session あたり 60 分上限があるため、常時動作を
成立させるには session の張り替えが必要になる。MVP では 50 分ごとに新しい session
を作り直す。

張り替え時は次の順で処理する。

1. 現在の audio buffer を commit する
2. completed event を一定時間待つ
3. 現 session を閉じる
4. 新しい transcription session を作る
5. 以後の音声 chunk を新 session に送る

境界の欠落対策は、まず「張り替え直前に commit してから切り替える」だけにする。
overlap と重複除去は、欠落が実測された場合に追加する。

## transcript 出力

MVP の主出力は Markdown と JSONL の 2 種類にする。

```text
~/Documents/Tategoto/
  2026-05-05.md
  2026-05-05.jsonl
```

Markdown は人間が読むための出力、JSONL は後から再処理するための構造化ログとして使う。

Markdown の例:

```md
# 2026-05-05

## 09:15

- [09:15:02-09:15:18] 今日の作業はまず設計書の見直しから始めます。
- [09:15:20-09:15:31] 常時文字起こしとファイル出力だけを先に作ります。
```

JSONL の例:

```jsonl
{"type":"transcript_segment","local_start":"2026-05-05T09:15:02+09:00","local_end":"2026-05-05T09:15:18+09:00","session_id":"sess_...","item_id":"item_...","previous_item_id":null,"model":"gpt-4o-transcribe","text":"今日の作業はまず設計書の見直しから始めます。","received_at":"2026-05-05T09:15:19+09:00"}
```

保存時に最低限持つ情報は次の通り。

- local start/end timestamp
- session id
- item id
- previous item id
- model name
- transcript text
- completion received timestamp

Realtime event の到着順だけを信用せず、後から timestamp と item relation で並べ直せる
形にしておく。

## UI

常時文字起こしアプリなので、MVP の主 UI はメニューバー常駐にする。Dock に常時出る
通常ウィンドウアプリより、録音状態を小さく確認でき、開始・停止しやすい方が用途に合う。

メニューバーには状態が分かるアイコンを出す。

- idle: 待機中
- recording: 録音中
- rotating_session: session 張り替え中
- stopped_with_error: 停止エラーあり

メニューバーのポップオーバーまたはメニューに置く操作は次の通り。

- Start / Stop
- 現在の状態
- 現在の入力デバイス
- 入力デバイスモードの切り替え
- 入力デバイス一覧からの固定デバイス選択
- 今日の出力ファイルを開く
- 出力ディレクトリを開く
- 最後のエラー表示
- Quit

MVP では transcript の閲覧・編集 UI は作らない。出力済み Markdown を開けば読める状態にする。
アプリ内表示を作るとしても、直近数件の transcript preview に留める。

React の画面は、メニューバーから開く小さな設定・状態ビューとして作る。UI の責務は
次に限定する。

- 現在状態の表示
- Start / Stop の発火
- 入力デバイス一覧の表示
- `system_default` / `fixed_device` の選択
- 出力先と最新 transcript の確認
- 最後のエラーの表示

録音中の状態遷移は Rust 側を正とし、React 側は Tauri event を購読して表示を更新する。

## アプリの状態

MVP では状態を増やしすぎない。UI またはログで確認できる状態は次の 4 つで足りる。

- `idle`: 文字起こししていない
- `recording`: マイク入力を取り、transcription session に送っている
- `rotating_session`: 50 分ごとの session 張り替え中
- `stopped_with_error`: 認証、接続、ファイル書き込みなどで停止した

`stopped_with_error` からの自動復旧はしない。原因を表示し、ユーザーが再起動する。

## 実装順

1. ChatGPT bearer token で transcription session を作れることを確認する
2. WebSocket に接続し、短い WAV または生成音声を送って transcript を得る
3. Tauri + React のアプリ shell とメニューバー常駐 UI を作る
4. Tauri command / event で Start / Stop と状態表示を接続する
5. Rust 側で入力デバイス一覧を取得し、`system_default` / `fixed_device` を保存できるようにする
6. 選択された Mac のマイク入力を PCM16 mono 24 kHz chunk として取り出す
7. 固定長 chunk を commit し、completed event を受け取る
8. Markdown と JSONL に追記する file writer を作る
9. 停止時に未送信 buffer を commit して終了する
10. 50 分ごとの session 張り替えを入れる

この順にすると、マイク入力、Realtime 接続、ファイル出力を個別に検証できる。

## 検証項目

- 5 分程度の手元発話が Markdown と JSONL に保存される
- `system_default` で録音開始時のデフォルト入力デバイスが使われる
- `fixed_device` で選択した入力デバイスが使われる
- 固定デバイスが外れている場合、デフォルトに fallback せずエラー停止する
- メニューバーから Start / Stop、出力ファイル確認、入力デバイス選択ができる
- 無音を含む状態でもアプリが落ちずに動き続ける
- 停止操作時に最後の chunk が保存される
- 60 分を超える運用で session 張り替え後も出力が続く
- ファイル書き込みに失敗した場合、エラー状態で停止する

認証切れの復旧、プライバシー UI、議事録生成はこの検証項目に含めない。

## 後で考えること

MVP が動いた後に、必要に応じて次を追加する。

- token 期限切れ時の再接続、再ログイン、ユーザー通知
- ローカル VAD、overlap、重複除去による transcript 品質改善
- transcript の DB 化、検索、編集 UI
- 5〜15 分ごとの小要約と議事録生成
- 保存期間、削除、録音中表示、送信制御などのプライバシー設計
- 録音中の入力デバイス変更の自動追従
- 話者分離モデルまたは別経路による speaker attribution
