import type { NoiseReductionType, TranscriptionModel } from "./types";

export const transcriptionModelOptions: Array<{
  value: TranscriptionModel;
  label: string;
}> = [
  { value: "gpt-4o-transcribe", label: "高精度 (gpt-4o-transcribe)" },
  { value: "gpt-4o-mini-transcribe", label: "軽量 (gpt-4o-mini-transcribe)" },
];

export const noiseReductionOptions: Array<{
  value: NoiseReductionType | "";
  label: string;
}> = [
  { value: "", label: "オフ" },
  { value: "near_field", label: "近接マイク向け" },
  { value: "far_field", label: "遠距離マイク向け" },
];

export const transcriptionLanguageOptions: Array<{
  value: string;
  label: string;
}> = [
  { value: "", label: "自動判定" },
  { value: "af", label: "Afrikaans (af)" },
  { value: "ar", label: "Arabic (ar)" },
  { value: "hy", label: "Armenian (hy)" },
  { value: "az", label: "Azerbaijani (az)" },
  { value: "be", label: "Belarusian (be)" },
  { value: "bs", label: "Bosnian (bs)" },
  { value: "bg", label: "Bulgarian (bg)" },
  { value: "ca", label: "Catalan (ca)" },
  { value: "zh", label: "Chinese (zh)" },
  { value: "hr", label: "Croatian (hr)" },
  { value: "cs", label: "Czech (cs)" },
  { value: "da", label: "Danish (da)" },
  { value: "nl", label: "Dutch (nl)" },
  { value: "en", label: "English (en)" },
  { value: "et", label: "Estonian (et)" },
  { value: "fi", label: "Finnish (fi)" },
  { value: "fr", label: "French (fr)" },
  { value: "gl", label: "Galician (gl)" },
  { value: "de", label: "German (de)" },
  { value: "el", label: "Greek (el)" },
  { value: "he", label: "Hebrew (he)" },
  { value: "hi", label: "Hindi (hi)" },
  { value: "hu", label: "Hungarian (hu)" },
  { value: "is", label: "Icelandic (is)" },
  { value: "id", label: "Indonesian (id)" },
  { value: "it", label: "Italian (it)" },
  { value: "ja", label: "日本語 (ja)" },
  { value: "kn", label: "Kannada (kn)" },
  { value: "kk", label: "Kazakh (kk)" },
  { value: "ko", label: "Korean (ko)" },
  { value: "lv", label: "Latvian (lv)" },
  { value: "lt", label: "Lithuanian (lt)" },
  { value: "mk", label: "Macedonian (mk)" },
  { value: "ms", label: "Malay (ms)" },
  { value: "mr", label: "Marathi (mr)" },
  { value: "mi", label: "Maori (mi)" },
  { value: "ne", label: "Nepali (ne)" },
  { value: "no", label: "Norwegian (no)" },
  { value: "fa", label: "Persian (fa)" },
  { value: "pl", label: "Polish (pl)" },
  { value: "pt", label: "Portuguese (pt)" },
  { value: "ro", label: "Romanian (ro)" },
  { value: "ru", label: "Russian (ru)" },
  { value: "sr", label: "Serbian (sr)" },
  { value: "sk", label: "Slovak (sk)" },
  { value: "sl", label: "Slovenian (sl)" },
  { value: "es", label: "Spanish (es)" },
  { value: "sw", label: "Swahili (sw)" },
  { value: "sv", label: "Swedish (sv)" },
  { value: "tl", label: "Tagalog (tl)" },
  { value: "ta", label: "Tamil (ta)" },
  { value: "th", label: "Thai (th)" },
  { value: "tr", label: "Turkish (tr)" },
  { value: "uk", label: "Ukrainian (uk)" },
  { value: "ur", label: "Urdu (ur)" },
  { value: "vi", label: "Vietnamese (vi)" },
  { value: "cy", label: "Welsh (cy)" },
];
