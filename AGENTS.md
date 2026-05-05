# AGENTS.md

tategoto(竪琴)はMacOSのマイク入力を常時文字起こしし、結果をローカルファイルへ保存する個人用アプリです。

## ドキュメント

現段階のドキュメントです。追加されたら随時AGENTS.mdを更新すること。

- [tategoto_app_design.md](tategoto_app_design.md): Tategotoのアプリ設計
- [chatgpt_realtime_transcription.md](chatgpt_realtime_transcription.md): ChatGPTのRealtime Transcription APIについて

## コミット

履歴は Conventional Commits 形式に従うこと。

- 形式: `<type>: <summary>`
- コミットメッセージは件名・本文ともに日本語で記載すること（Conventional Commits の `type` は英語のままで可）。
- 3行目以降には具体的な変更内容を記載すること。
- コミットメッセージ本文に改行を入れるときは、`-m` 引数内に `\n` を書かないこと。`git commit -m "<件名>" -m "<本文>"` のように `-m` を分けるか、メッセージファイルを使うこと。
- 1コミット1目的を徹底すること。
- 適切な粒度でコミットを行うこと。概ね1コミットは100行程度が望ましい(自動生成ファイルやlockファイルは含まない)。
