// The only module that talks to the backend. Swap `invoke` for `fetch` here to
// run the same UI against a web server later.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** A local user. Each has their own songs, grades, Repaso and tutor. */
export interface User {
  id: string;
  name: string;
}

export interface UserList {
  current: string;
  users: User[];
}

export interface Grade {
  score: number;
  verdict: string;
  missed_nuance: string[];
  best_translation: string;
  skill_tags: string[];
}

export interface Attempt {
  id: number;
  line_id: number;
  translation: string;
  grade: Grade;
  source: "board" | "repaso";
  created_at: string;
  /** The line's Spanish when graded; differs from the line if it was edited since. */
  line_text: string | null;
}

export interface EditPlan {
  kept: number;
  changed: number;
  added: number;
  removed: number;
  /** Removed lines that have grades; those grades are deleted with them. */
  removed_graded: number;
}

export interface Line {
  id: number;
  idx: number;
  stanza: number;
  text: string;
  repeat_of: number | null;
  latest: Attempt | null;
  attempt_count: number;
}

export interface Song {
  id: number;
  title: string;
  artist: string;
  created_at: string;
  lines: Line[];
}

export interface SongSummary {
  id: number;
  title: string;
  artist: string;
  created_at: string;
  line_count: number;
  graded_count: number;
  avg_score: number | null;
}

export interface ThreadMessage {
  id: number;
  attempt_id: number;
  role: "user" | "assistant";
  content: string;
  created_at: string;
}

export interface TutorMessage {
  id: number;
  role: "user" | "assistant";
  content: string;
  created_at: string;
}

export interface TagStat {
  tag: string;
  count: number;
  avg_score: number;
}

export interface Stats {
  songs: number;
  lines_graded: number;
  attempts: number;
  avg_score: number | null;
  score_counts: number[];
  tags: TagStat[];
  due_count: number;
}

export interface ReviewItem {
  song_id: number;
  song_title: string;
  artist: string;
  line: Line;
}

export interface LyricsMatch {
  id: number;
  track: string;
  artist: string;
  album: string;
  duration_secs: number;
  lyrics: string;
}

export interface LookupResult {
  matches: LyricsMatch[];
  /** False when nothing matched the artist given and these are title-only matches. */
  artist_matched: boolean;
}

export interface Word {
  text: string;
  start: number;
  end: number;
  prob: number;
}

export interface DraftLine {
  text: string;
  start: number;
  end: number;
  stanza: number;
  words: Word[];
  flags: ("unsure" | "no-speech" | "stock-phrase" | "loop")[];
}

export interface Draft {
  video_id: string;
  url: string;
  title: string;
  artist: string;
  method: "youtube-subs" | "whisper";
  audio_path: string;
  vocals_path: string | null;
  device: string | null;
  lines: DraftLine[];
}

export interface TranscribeProgress {
  stage: "metadata" | "download" | "captions" | "separate" | "transcribe" | "done";
  message: string;
  fraction: number | null;
}

export interface Suggestion {
  line: number;
  original: string;
  replacement: string;
  reason: string;
}

export interface ChatTurn {
  role: "user" | "assistant";
  content: string;
}

export interface Proofread {
  suggestions: Suggestion[];
  suspicious_lines: { line: number; reason: string }[];
  rejected: number;
}

/** Below this Whisper probability a word is marked unsure (matches the backend). */
export const UNSURE_BELOW = 0.5;

export const api = {
  hasApiKey: () => invoke<boolean>("has_api_key"),
  setApiKey: (key: string) => invoke<void>("set_api_key", { key }),

  listUsers: () => invoke<UserList>("list_users"),
  switchUser: (id: string) => invoke<UserList>("switch_user", { id }),

  listSongs: () => invoke<SongSummary[]>("list_songs"),
  getSong: (id: number) => invoke<Song>("get_song", { id }),
  deleteSong: (id: number) => invoke<void>("delete_song", { id }),
  /** `apply: false` only reports what would change. */
  updateSong: (id: number, title: string, artist: string, lyrics: string, apply: boolean) =>
    invoke<EditPlan>("update_song", { id, title, artist, lyrics, apply }),
  /** Fixes one line; lines that repeat it change too. */
  editLine: (lineId: number, text: string) => invoke<void>("edit_line", { lineId, text }),
  /** `source` is null for pasted lyrics, else lrclib:<id>, youtube-subs:<id> or whisper:<id>. */
  createSong: (title: string, artist: string, lyrics: string, source: string | null) =>
    invoke<number>("create_song", { title, artist, lyrics, source }),
  lookupLyrics: (track: string, artist: string) => invoke<LookupResult>("lookup_lyrics", { track, artist }),

  submitTranslation: (lineId: number, translation: string, source: "board" | "repaso") =>
    invoke<Attempt>("submit_translation", { lineId, translation, source }),
  /** Grades the same translation again, replacing that attempt's grade (not a new attempt). */
  regradeAttempt: (attemptId: number) => invoke<Attempt>("regrade_attempt", { attemptId }),
  getThread: (attemptId: number) => invoke<ThreadMessage[]>("get_thread", { attemptId }),
  askFollowup: (attemptId: number, question: string) =>
    invoke<ThreadMessage[]>("ask_followup", { attemptId, question }),

  transcribeYoutube: (url: string) => invoke<Draft>("transcribe_youtube", { url }),
  onTranscribeProgress: (cb: (p: TranscribeProgress) => void): Promise<UnlistenFn> =>
    listen<TranscribeProgress>("transcribe-progress", (e) => cb(e.payload)),
  proofread: (title: string, artist: string, lines: DraftLine[]) =>
    invoke<Proofread>("proofread_transcript", { title, artist, lines }),
  /** Claude's reply in a conversation about one line of a transcript. */
  discussLine: (
    title: string,
    artist: string,
    lines: DraftLine[],
    line: number,
    suggestions: Suggestion[],
    suspicious: string | null,
    history: ChatTurn[],
    question: string,
  ) =>
    invoke<string>("discuss_line", { title, artist, lines, line, suggestions, suspicious, history, question }),
  /** Writes a diagnostic line to the app's client.log. Never throws. */
  logClient: (message: string) => invoke<void>("log_client", { message }).catch(() => {}),
  /** Whether the webview can play audio (on Linux, needs gst-plugins-good). */
  audioSupport: () => invoke<{ ok: boolean; missing: string[] }>("audio_support"),
  /** A blob URL for a WAV the backend downloaded. The caller revokes it when done. */
  audioUrl: async (path: string) => {
    const bytes = await invoke<ArrayBuffer>("read_audio", { path });
    return URL.createObjectURL(new Blob([bytes], { type: "audio/wav" }));
  },

  tutorThread: () => invoke<TutorMessage[]>("tutor_thread"),
  askTutor: (question: string) => invoke<TutorMessage[]>("ask_tutor", { question }),
  clearTutor: () => invoke<void>("clear_tutor"),

  getStats: () => invoke<Stats>("get_stats"),
  reviewQueue: (tag: string | null) => invoke<ReviewItem[]>("review_queue", { tag }),
};

export function errorMessage(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
}
