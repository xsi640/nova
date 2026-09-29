import { synthesizeSpeechStream, type SpeechOptions } from "./commands";

export interface StreamingSpeech {
  /** Stops synthesis playback early; `done` resolves. */
  stop(): void;
  /** Resolves when playback finishes; rejects when synthesis or playback fails. */
  done: Promise<void>;
}

/**
 * Plays `text` through Media Source Extensions, so the first words are heard while the rest of the
 * utterance is still being synthesized. Both providers return MP3, which Chromium plays from a
 * `audio/mpeg` SourceBuffer.
 */
export function playStreamingSpeech(
  text: string,
  options: SpeechOptions | undefined,
): StreamingSpeech {
  let audio: HTMLAudioElement | null = null;
  let mediaSource: MediaSource | null = null;
  let sourceBuffer: SourceBuffer | null = null;
  let objectUrl: string | null = null;
  const queue: ArrayBuffer[] = [];
  let finished = false;
  let stopped = false;
  let resolveDone!: () => void;
  let rejectDone!: (error: unknown) => void;
  const done = new Promise<void>((resolve, reject) => {
    resolveDone = resolve;
    rejectDone = reject;
  });

  function flush() {
    if (stopped || !sourceBuffer || sourceBuffer.updating) return;
    const chunk = queue.shift();
    if (chunk) {
      try {
        sourceBuffer.appendBuffer(chunk);
      } catch (error) {
        rejectDone(error);
      }
      return;
    }
    if (finished && mediaSource && mediaSource.readyState === "open") {
      try {
        mediaSource.endOfStream();
      } catch {
        // The stream was already ended.
      }
    }
  }

  function startPlayer(contentType: string) {
    if (audio || stopped) return;
    mediaSource = new MediaSource();
    objectUrl = URL.createObjectURL(mediaSource);
    audio = new Audio(objectUrl);
    audio.onended = () => resolveDone();
    audio.onerror = () => rejectDone(new Error("语音播放失败，请重试或检查语音服务设置"));
    mediaSource.addEventListener(
      "sourceopen",
      () => {
        if (!mediaSource || stopped) return;
        sourceBuffer = mediaSource.addSourceBuffer(contentType);
        sourceBuffer.mode = "sequence";
        sourceBuffer.addEventListener("updateend", flush);
        flush();
      },
      { once: true },
    );
    void audio.play().catch((error) => rejectDone(error));
  }

  void synthesizeSpeechStream(text, options, (event) => {
    if (stopped) return;
    if (event.type === "start") {
      startPlayer(event.contentType);
    } else {
      queue.push(base64ToBytes(event.data));
      flush();
    }
  })
    .then(() => {
      finished = true;
      if (!stopped) flush();
    })
    .catch((error) => {
      if (!stopped) rejectDone(error);
    });

  return {
    stop() {
      if (stopped) return;
      stopped = true;
      queue.length = 0;
      if (audio) {
        audio.onended = null;
        audio.onerror = null;
        audio.pause();
        audio.src = "";
      }
      if (objectUrl) URL.revokeObjectURL(objectUrl);
      objectUrl = null;
      resolveDone();
    },
    done,
  };
}

function base64ToBytes(value: string): ArrayBuffer {
  const binary = atob(value);
  const buffer = new ArrayBuffer(binary.length);
  const bytes = new Uint8Array(buffer);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return buffer;
}

export interface LiveSpeech {
  /** Appends a streamed text delta; complete sentences are spoken as they arrive. */
  push(text: string): void;
  /** Flushes the trailing text and lets the queue drain. */
  finish(): void;
  /** Stops playback and drops anything still queued. */
  stop(): void;
  /** Resolves when the queue has drained. */
  done: Promise<void>;
}

const SENTENCE_ENDERS = "。！？!?；;\n";
const MIN_SEGMENT_LENGTH = 24;
const MAX_SEGMENT_LENGTH = 200;

/**
 * Speaks a model reply while it is still being generated. Deltas are buffered into sentences and
 * each completed sentence is synthesized and played in order, so the companion starts talking
 * before the full answer exists.
 */
export function createLiveSpeech(options: SpeechOptions | undefined): LiveSpeech {
  let buffer = "";
  const queue: string[] = [];
  let finished = false;
  let stopped = false;
  let running = false;
  let current: StreamingSpeech | null = null;
  let resolveDone!: () => void;
  const done = new Promise<void>((resolve) => {
    resolveDone = resolve;
  });

  async function run() {
    if (running) return;
    running = true;
    while (!stopped) {
      const next = queue.shift();
      if (next === undefined) break;
      const speech = playStreamingSpeech(next, options);
      current = speech;
      try {
        await speech.done;
      } catch {
        // One failed sentence should not abort the rest of the reply.
      }
      current = null;
    }
    running = false;
    if (stopped || (finished && queue.length === 0)) resolveDone();
  }

  function enqueue(text: string) {
    const trimmed = text.trim();
    if (!trimmed) return;
    queue.push(trimmed);
    void run();
  }

  function drainBuffer() {
    while (!stopped) {
      let cut = -1;
      for (let index = MIN_SEGMENT_LENGTH - 1; index < buffer.length; index += 1) {
        const character = buffer[index];
        if (character !== undefined && SENTENCE_ENDERS.includes(character)) {
          cut = index;
          break;
        }
      }
      if (cut < 0) {
        if (buffer.length < MAX_SEGMENT_LENGTH) break;
        const window = buffer.slice(0, MAX_SEGMENT_LENGTH);
        const soft = Math.max(
          window.lastIndexOf("，"),
          window.lastIndexOf(","),
          window.lastIndexOf("、"),
          window.lastIndexOf(" "),
        );
        cut = soft > 0 ? soft : MAX_SEGMENT_LENGTH - 1;
      }
      enqueue(buffer.slice(0, cut + 1));
      buffer = buffer.slice(cut + 1);
    }
  }

  return {
    push(text) {
      if (stopped || finished) return;
      buffer += text;
      drainBuffer();
    },
    finish() {
      if (stopped || finished) return;
      finished = true;
      if (buffer.trim()) {
        enqueue(buffer);
        buffer = "";
      }
      if (!running) resolveDone();
    },
    stop() {
      if (stopped) return;
      stopped = true;
      queue.length = 0;
      current?.stop();
      resolveDone();
    },
    done,
  };
}
