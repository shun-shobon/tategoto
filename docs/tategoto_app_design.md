# Tategoto アプリ設計メモ

Tategoto は、macOS のマイク入力を常時文字起こしし、結果をローカルファイルへ保存する
個人用アプリである。現在の到達点は「メニューバー常駐アプリから録音を開始し、Apple
SpeechTranscriber の final result を日別 Markdown / JSONL に追記すること」とする。

この段階では、古い macOS への fallback、話者分離、議事録生成、検索、同期は扱わない。
音声入力、Speech 権限、モデル資産、ファイル書き込みのいずれかが失敗した場合は
`stopped_with_error` に遷移し、ユーザーが手元で復旧してから再度開始する。

## 現状のゴール

- macOS のメニューバーに常駐し、録音状態を確認できる
- メインウィンドウから Start / Stop、入力デバイス、出力先を操作できる
- 入力デバイスをシステムデフォルト追従または特定デバイス固定から選べる
- Rust 側でマイク入力を PCM16 mono 24 kHz に変換する
- Swift bridge で Apple SpeechAnalyzer / SpeechTranscriber に音声を渡す
- SpeechTranscriber の final result をローカルファイルへ追記する
- 日別の Markdown と JSONL を `~/Documents/Tategoto/` に残す

MVP では「録音・文字起こし・保存の経路が単純で、失敗時に状態が分かること」を優先する。
リアルタイム preview、検索、編集、要約、復旧導線の作り込みは後回しにする。

## 非ゴール

- macOS 26.0 未満への fallback
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
  -> Swift Apple SpeechTranscriber bridge
  -> Rust file writer
```

React 側は状態表示と操作に寄せる。長時間動作する録音・文字起こし・保存のライフサイクルは
Rust 側に置き、Apple の Speech framework との接続だけを Swift bridge に閉じる。

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
  -> Apple Speech bridge lifecycle
  -> transcript file writer

Swift bridge
  -> SFSpeechRecognizer authorization
  -> SpeechTranscriber locale support check
  -> AssetInventory model installation
  -> SpeechAnalyzer input stream
  -> final result event emission
```

## 動作環境と権限

対象 OS は macOS 26.0 以上に固定する。SpeechTranscriber / SpeechAnalyzer /
AssetInventory が macOS 26.0 以降の API であるため、古い OS 向けの代替経路は持たない。

`Info.plist` には次の権限文言を持つ。

- `NSMicrophoneUsageDescription`
- `NSSpeechRecognitionUsageDescription`

録音開始時に Speech recognition authorization を確認し、未決定なら OS の許可 prompt を出す。
拒否・制限・未対応の場合は `stopped_with_error` にする。

`pnpm tauri dev` の未バンドル実行体では macOS TCC が Speech Recognition の権限
prompt を安全に出せないため、未許可状態では録音開始前にエラー停止する。初回の権限付与は
`pnpm tauri build --bundles app` で作成した `Tategoto.app` から行う。

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
    "locale_identifier": "ja-JP"
  }
}
```

`input_device_name` は表示用であり、録音に使う識別子は `input_device_id` とする。現在の
`input_device_id` は cpal が返す入力デバイスの列挙順と名前から作る hash であり、OS の
永続的な hardware id ではない。

`system_default` の場合は、録音開始時にその時点のデフォルト入力デバイスを解決する。
録音中に macOS のデフォルト入力が変わった場合の自動追従はしない。

`fixed_device` の場合は、録音開始時に保存済み device id を探す。見つからない場合は
勝手にデフォルトへ落とさず、`stopped_with_error` として停止する。

## 文字起こし設定

設定できる項目は Apple SpeechTranscriber に渡す認識ロケールだけにする。未指定の場合は
現在の locale を渡し、対応する locale が見つからなければ開始失敗にする。

SpeechTranscriber の volatile result は保存せず、final result だけを保存対象にする。

## 音声入力と Speech bridge

Rust 側で cpal の入力 stream を作り、デバイスの default input config に従って受け取った
サンプルを PCM16 mono 24 kHz に変換する。対応している input sample format は `f32`、
`i16`、`u16` である。各 audio block には取得時刻と音声長を持たせる。

Swift bridge は PCM16 block を `AVAudioPCMBuffer` に変換し、
`SpeechAnalyzer.bestAvailableAudioFormat` が返す形式へ `AVAudioConverter` で変換してから
`AnalyzerInput` として投入する。`bufferStartTime` は送信済み sample 数から作る。

録音停止時は Rust 側で audio queue を drain し、Swift bridge の input stream を finish する。
Swift bridge は `finalizeAndFinishThroughEndOfInput()` を呼び、最後の final result を流してから
停止イベントを返す。停止完了が 10 秒以内に返らない場合はエラーとして扱う。

## transcript 出力

主出力は Markdown と JSONL の 2 種類にする。

```text
~/Documents/Tategoto/
  2026-05-05.md
  2026-05-05.jsonl
```

Documents directory が取れない場合は home directory 配下の `Tategoto` を使う。

Markdown の例:

```md
# 2026-05-05

## 09:15
- [09:15:02-09:15:18] 今日の作業はまず設計書の見直しから始めます。
```

JSONL の例:

```jsonl
{"type":"transcript_segment","local_start":"2026-05-05T09:15:02+09:00","local_end":"2026-05-05T09:15:18+09:00","session_id":"apple-session","item_id":"apple_1","previous_item_id":null,"text":"今日の作業はまず設計書の見直しから始めます。","received_at":"2026-05-05T09:15:19+09:00"}
```

local start/end timestamp は、SpeechTranscriber result の audio offset を、Rust 側で記録した
audio block の取得時刻へ対応付けて作る。Mac のスリープなどで wall-clock time と音声
offset がずれた場合も、復帰後の発話は復帰後の実時刻として保存する。

## UI

主 UI はメニューバー常駐にする。Dock に常時出る通常ウィンドウアプリより、録音状態を
小さく確認でき、開始・停止しやすい方が用途に合う。

メニューバーには状態が分かる title / tooltip を出す。

- `idle`: `○ 待機中`
- `recording`: `● 録音中`
- `stopped_with_error`: `! エラー`

React の画面の責務は次に限定する。

- 現在状態の表示
- Start / Stop の発火
- 入力デバイス一覧の表示と再取得
- `system_default` / `fixed_device` の選択
- 文字起こし言語の選択
- 今日の Markdown / JSONL path と出力 directory の表示
- 今日の Markdown と出力 directory を開く
- 最後のエラーの表示

## 検証項目

- `system_default` で録音開始時のデフォルト入力デバイスが使われる
- `fixed_device` で選択した入力デバイスが使われる
- 固定デバイスが外れている場合、デフォルトに fallback せずエラー停止する
- Speech recognition 権限がない場合、エラー状態で停止する
- SpeechTranscriber の locale が未対応の場合、エラー状態で停止する
- モデル資産が未導入の場合、AssetInventory 経由で導入してから録音を開始する
- React window から Start / Stop、出力ファイル確認、入力デバイス選択ができる
- 5 分程度の手元発話が Markdown と JSONL に保存される
- 無音を含む状態でもアプリが落ちずに動き続ける
- 停止操作時に最後の未確定発話が保存される
- ファイル書き込みに失敗した場合、エラー状態で停止する
