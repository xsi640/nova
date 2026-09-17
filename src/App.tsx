import { useEffect, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import {
  bootstrap,
  getApiProfileStatus,
  getPersona,
  getSettings,
  confirmSchedule,
  deleteMemory,
  deleteSchedule,
  exportLocalData,
  getScheduleCandidate,
  listMemories,
  listMessages,
  listSchedules,
  retryMessage,
  saveApiProfile,
  savePersona,
  saveSettings,
  sendMessage,
  setWindowMode,
  showNotification,
  synthesizeSpeech,
  testApiProfile,
  transcribeAudio,
  type ApiCapability,
  type ApiProfileInput,
  type ApiProfileStatus,
  type AppSettings,
  type AppError,
  type BootstrapResponse,
  type ChatExchange,
  type ChatMessage,
  type ConfirmScheduleInput,
  type MemoryRecord,
  type PersonaProfile,
  type ScheduleRecord,
  type ScheduleCandidate,
  type ScheduleStatus,
  updateMemory,
  updateSchedule,
  type WindowMode,
} from "./lib/commands";

type Page = "chat" | "memory" | "schedule" | "settings";

const navigation: Array<{ id: Page; label: string; glyph: string }> = [
  { id: "chat", label: "聊天", glyph: "✦" },
  { id: "memory", label: "记忆", glyph: "◇" },
  { id: "schedule", label: "日程", glyph: "□" },
  { id: "settings", label: "设置", glyph: "⚙" },
];

const defaultSettings: AppSettings = {
  theme: "rose",
  darkMode: false,
  dndStart: "23:00",
  dndEnd: "08:00",
  voiceAutoplay: true,
  proactiveEnabled: true,
};

const defaultPersona: PersonaProfile = {
  name: "小诺",
  personality: "温柔、爱倾听、有一点俏皮",
  speechStyle: "自然、轻松，偶尔带一点可爱的语气",
};

const apiCapabilityMeta: Record<ApiCapability, { label: string; path: string; model: string }> = {
  chat: { label: "对话", path: "/chat/completions", model: "gpt-4.1-mini" },
  transcription: { label: "语音识别", path: "/audio/transcriptions", model: "gpt-4o-mini-transcribe" },
  speech: { label: "语音合成", path: "/audio/speech", model: "gpt-4o-mini-tts" },
};

function emptyApiProfile(capability: ApiCapability): ApiProfileInput {
  const defaults = apiCapabilityMeta[capability];
  return {
    capability,
    baseUrl: "https://api.openai.com/v1",
    path: defaults.path,
    model: defaults.model,
    apiKey: null,
    enabled: true,
  };
}

function appError(error: unknown): AppError | null {
  if (typeof error === "string") {
    try {
      return appError(JSON.parse(error));
    } catch {
      return null;
    }
  }
  if (
    typeof error === "object" && error && "message" in error && "retryable" in error
  ) {
    return error as AppError;
  }
  return null;
}

function errorMessage(error: unknown): string {
  const nativeError = appError(error);
  if (nativeError) return nativeError.message;
  if (typeof error === "object" && error && "message" in error) {
    return String(error.message);
  }
  return "保存失败，请稍后重试";
}

function StatusPill({ status }: { status: BootstrapResponse | null }) {
  const ready = status?.databaseReady ?? false;
  return (
    <span className={ready ? "status status--ready" : "status"}>
      <span className="status__dot" />
      {ready ? "本地数据已就绪" : "开发预览"}
    </span>
  );
}

function WindowModeButton({ mode, onChange }: { mode: WindowMode; onChange: () => void }) {
  return (
    <button className="window-mode" type="button" onClick={onChange}>
      {mode === "compact" ? "打开管理页" : "切换聊天浮窗"}
    </button>
  );
}

function toLocalDateTimeValue(value: string | Date): string {
  const date = typeof value === "string" ? new Date(value) : value;
  const padded = (part: number) => String(part).padStart(2, "0");
  return `${date.getFullYear()}-${padded(date.getMonth() + 1)}-${padded(date.getDate())}T${padded(date.getHours())}:${padded(date.getMinutes())}`;
}

function defaultDateTimeValue(): string {
  const date = new Date();
  date.setMinutes(0, 0, 0);
  date.setHours(date.getHours() + 1);
  return toLocalDateTimeValue(date);
}

function displayDateTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat("zh-CN", {
    month: "long", day: "numeric", weekday: "short", hour: "2-digit", minute: "2-digit",
  }).format(date);
}

function scheduleDay(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "未指定日期";
  const today = new Date();
  const tomorrow = new Date();
  tomorrow.setDate(today.getDate() + 1);
  const sameDay = (left: Date, right: Date) => left.getFullYear() === right.getFullYear()
    && left.getMonth() === right.getMonth() && left.getDate() === right.getDate();
  if (sameDay(date, today)) return "今天";
  if (sameDay(date, tomorrow)) return "明天";
  return new Intl.DateTimeFormat("zh-CN", { year: "numeric", month: "long", day: "numeric", weekday: "short" }).format(date);
}

function MemoryPage() {
  const [memories, setMemories] = useState<MemoryRecord[]>([]);
  const [loading, setLoading] = useState(true);
  const [notice, setNotice] = useState<string | null>(null);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [draft, setDraft] = useState("");
  const [busyId, setBusyId] = useState<number | null>(null);

  useEffect(() => {
    let active = true;
    listMemories()
      .then((saved) => { if (active) setMemories(saved); })
      .catch((error) => { if (active) setNotice(errorMessage(error)); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, []);

  function beginEdit(memory: MemoryRecord) {
    setEditingId(memory.id);
    setDraft(memory.content);
    setNotice(null);
  }

  async function saveMemory(memory: MemoryRecord) {
    const content = draft.trim();
    if (!content) {
      setNotice("记忆内容不能为空");
      return;
    }
    setBusyId(memory.id);
    setNotice(null);
    try {
      const saved = await updateMemory(memory.id, content);
      if (!saved) throw new Error("这条记忆已不存在，请刷新后重试");
      setMemories((current) => current.map((item) => item.id === saved.id ? saved : item));
      setEditingId(null);
    } catch (error) {
      setNotice(errorMessage(error));
    } finally {
      setBusyId(null);
    }
  }

  async function removeMemory(memory: MemoryRecord) {
    if (!window.confirm("删除这条记忆？此操作无法撤销。")) return;
    setBusyId(memory.id);
    setNotice(null);
    try {
      await deleteMemory(memory.id);
      setMemories((current) => current.filter((item) => item.id !== memory.id));
      if (editingId === memory.id) setEditingId(null);
    } catch (error) {
      setNotice(errorMessage(error));
    } finally {
      setBusyId(null);
    }
  }

  return (
    <section className="management-page">
      <header className="page-heading">
        <div><p className="eyebrow">长期记忆</p><h1>记忆</h1><p>这里只保留从对话中确认下来的重要信息。</p></div>
        <span className="save-state">{memories.length} 条记忆</span>
      </header>
      {notice && <p className="page-notice" role="alert">{notice}</p>}
      {loading ? <p className="management-loading">正在读取记忆…</p> : memories.length === 0 ? (
        <section className="empty-management"><span>◇</span><h2>还没有保存的记忆</h2><p>聊天中被确认的重要信息会自动出现在这里。</p></section>
      ) : (
        <div className="memory-list">
          {memories.map((memory) => {
            const editing = editingId === memory.id;
            const busy = busyId === memory.id;
            return <article className="management-card memory-card" key={memory.id}>
              <div className="management-card__body">
                {editing ? <textarea aria-label="编辑记忆" autoFocus disabled={busy} onChange={(event) => setDraft(event.target.value)} rows={3} value={draft} /> : <p>{memory.content}</p>}
                <small>来自聊天消息 #{memory.sourceMessageId} · 更新于 {displayDateTime(memory.updatedAt)}</small>
              </div>
              <div className="management-card__actions">
                {editing ? <>
                  <button className="secondary-button" disabled={busy} onClick={() => setEditingId(null)} type="button">取消</button>
                  <button className="primary-button" disabled={busy} onClick={() => void saveMemory(memory)} type="button">{busy ? "保存中…" : "保存"}</button>
                </> : <>
                  <button className="text-button" disabled={busy} onClick={() => beginEdit(memory)} type="button">编辑</button>
                  <button className="text-button text-button--danger" disabled={busy} onClick={() => void removeMemory(memory)} type="button">删除</button>
                </>}
              </div>
            </article>;
          })}
        </div>
      )}
    </section>
  );
}

type ScheduleDraft = ConfirmScheduleInput & { id?: number; status: ScheduleStatus };

function newScheduleDraft(): ScheduleDraft {
  const scheduledAt = defaultDateTimeValue();
  return { title: "", scheduledAt, remindAt: scheduledAt, status: "scheduled" };
}

function SchedulePage() {
  const [schedules, setSchedules] = useState<ScheduleRecord[]>([]);
  const [loading, setLoading] = useState(true);
  const [notice, setNotice] = useState<string | null>(null);
  const [draft, setDraft] = useState<ScheduleDraft>(newScheduleDraft);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [saving, setSaving] = useState(false);
  const [deletingId, setDeletingId] = useState<number | null>(null);

  useEffect(() => {
    let active = true;
    listSchedules()
      .then((saved) => { if (active) setSchedules(saved); })
      .catch((error) => { if (active) setNotice(errorMessage(error)); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, []);

  function editSchedule(schedule: ScheduleRecord) {
    setEditingId(schedule.id);
    setDraft({
      id: schedule.id, title: schedule.title, scheduledAt: toLocalDateTimeValue(schedule.scheduledAt),
      remindAt: toLocalDateTimeValue(schedule.remindAt), sourceMessageId: schedule.sourceMessageId ?? undefined, status: schedule.status,
    });
    setNotice(null);
  }

  function resetForm() {
    setEditingId(null);
    setDraft(newScheduleDraft());
  }

  async function saveSchedule(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const title = draft.title.trim();
    if (!title || !draft.scheduledAt || !draft.remindAt) {
      setNotice("请填写事项、日程时间和提醒时间");
      return;
    }
    setSaving(true);
    setNotice(null);
    try {
      if (editingId !== null) {
        const saved = await updateSchedule({ id: editingId, title, scheduledAt: draft.scheduledAt, remindAt: draft.remindAt, status: draft.status });
        if (!saved) throw new Error("该日程已不存在，请刷新后重试");
        setSchedules((current) => current.map((item) => item.id === saved.id ? saved : item));
      } else {
        const saved = await confirmSchedule({ title, scheduledAt: draft.scheduledAt, remindAt: draft.remindAt, sourceMessageId: draft.sourceMessageId });
        setSchedules((current) => [...current, saved].sort((left, right) => left.scheduledAt.localeCompare(right.scheduledAt)));
      }
      resetForm();
    } catch (error) {
      setNotice(errorMessage(error));
    } finally {
      setSaving(false);
    }
  }

  async function removeSchedule(schedule: ScheduleRecord) {
    if (!window.confirm(`删除“${schedule.title}”？此操作无法撤销。`)) return;
    setDeletingId(schedule.id);
    setNotice(null);
    try {
      await deleteSchedule(schedule.id);
      setSchedules((current) => current.filter((item) => item.id !== schedule.id));
      if (editingId === schedule.id) resetForm();
    } catch (error) {
      setNotice(errorMessage(error));
    } finally {
      setDeletingId(null);
    }
  }

  const groupedSchedules = schedules.reduce<Array<{ day: string; schedules: ScheduleRecord[] }>>((groups, schedule) => {
    const day = scheduleDay(schedule.scheduledAt);
    const group = groups.at(-1);
    if (group?.day === day) group.schedules.push(schedule);
    else groups.push({ day, schedules: [schedule] });
    return groups;
  }, []);

  return (
    <section className="management-page">
      <header className="page-heading">
        <div><p className="eyebrow">应用内日程</p><h1>日程</h1><p>通过聊天确认的约定会保存在这里，不会读取系统日历。</p></div>
        <span className="save-state">{schedules.length} 个日程</span>
      </header>
      {notice && <p className="page-notice" role="alert">{notice}</p>}
      <form className="schedule-form management-card" onSubmit={(event) => void saveSchedule(event)}>
        <div className="schedule-form__heading"><div><span className="section-tag">{editingId === null ? "新建日程" : "编辑日程"}</span><h2>{editingId === null ? "确认后创建日程" : "调整这个约定"}</h2></div>{editingId !== null && <button className="text-button" onClick={resetForm} type="button">取消编辑</button>}</div>
        <div className="schedule-form__fields">
          <label><span>事项</span><input maxLength={160} onChange={(event) => setDraft({ ...draft, title: event.target.value })} placeholder="例如：项目评审会" value={draft.title} /></label>
          <label><span>日程时间</span><input onChange={(event) => setDraft({ ...draft, scheduledAt: event.target.value })} required type="datetime-local" value={draft.scheduledAt} /></label>
          <label><span>提前提醒</span><input onChange={(event) => setDraft({ ...draft, remindAt: event.target.value })} required type="datetime-local" value={draft.remindAt} /></label>
          {editingId !== null && <label><span>状态</span><select onChange={(event) => setDraft({ ...draft, status: event.target.value })} value={draft.status}><option value="scheduled">待进行</option><option value="completed">已完成</option><option value="cancelled">已取消</option></select></label>}
        </div>
        <div className="schedule-form__actions"><small>{editingId === null ? "创建即表示已确认；对话中创建时会先请你确认。" : "修改会即时保存到本机。"}</small><button className="primary-button" disabled={saving} type="submit">{saving ? "保存中…" : editingId === null ? "确认并创建" : "保存修改"}</button></div>
      </form>
      {loading ? <p className="management-loading">正在读取日程…</p> : groupedSchedules.length === 0 ? (
        <section className="empty-management"><span>□</span><h2>还没有已确认的日程</h2><p>在聊天中提出约定，确认后就会显示在这里。</p></section>
      ) : <div className="schedule-groups">{groupedSchedules.map((group) => <section className="schedule-group" key={group.day}><h2>{group.day}</h2>{group.schedules.map((schedule) => <article className="management-card schedule-card" key={schedule.id}><div className="schedule-card__time"><strong>{displayDateTime(schedule.scheduledAt)}</strong><small>提醒：{displayDateTime(schedule.remindAt)}</small></div><div className="management-card__body"><p>{schedule.title}</p><small>{schedule.status === "completed" ? "已完成" : schedule.status === "cancelled" ? "已取消" : "待进行"}{schedule.sourceMessageId ? ` · 来自消息 #${schedule.sourceMessageId}` : ""}</small></div><div className="management-card__actions"><button className="text-button" onClick={() => editSchedule(schedule)} type="button">编辑</button><button className="text-button text-button--danger" disabled={deletingId === schedule.id} onClick={() => void removeSchedule(schedule)} type="button">{deletingId === schedule.id ? "删除中…" : "删除"}</button></div></article>)}</section>)}</div>}
    </section>
  );
}

function ChatPage({
  status,
  persona,
  windowMode,
  onWindowModeChange,
  onOpenSettings,
  settings,
}: {
  status: BootstrapResponse | null;
  persona: PersonaProfile | null;
  windowMode: WindowMode;
  onWindowModeChange: () => void;
  onOpenSettings: () => void;
  settings: AppSettings;
}) {
  const name = persona?.name ?? "Nova";
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [draft, setDraft] = useState("");
  const [loadingHistory, setLoadingHistory] = useState(true);
  const [sending, setSending] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [recording, setRecording] = useState(false);
  const [transcribing, setTranscribing] = useState(false);
  const [transcriptDraft, setTranscriptDraft] = useState<string | null>(null);
  const [speakingMessageId, setSpeakingMessageId] = useState<number | null>(null);
  const [scheduleCandidate, setScheduleCandidate] = useState<ScheduleCandidate | null>(null);
  const [confirmingSchedule, setConfirmingSchedule] = useState(false);
  const conversationRef = useRef<HTMLDivElement>(null);
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const recorderRef = useRef<MediaRecorder | null>(null);
  const recordingStreamRef = useRef<MediaStream | null>(null);
  const recordingChunksRef = useRef<Blob[]>([]);
  const audioRef = useRef<HTMLAudioElement | null>(null);

  useEffect(() => {
    let active = true;
    listMessages()
      .then((savedMessages) => {
        if (active) setMessages(savedMessages);
      })
      .catch(() => {
        // Vite's browser preview does not expose the Tauri IPC bridge.
      })
      .finally(() => {
        if (active) setLoadingHistory(false);
      });
    return () => { active = false; };
  }, []);

  useEffect(() => {
    conversationRef.current?.scrollTo({ top: conversationRef.current.scrollHeight, behavior: "smooth" });
  }, [messages, sending]);

  useEffect(() => () => {
    recorderRef.current?.stop();
    recordingStreamRef.current?.getTracks().forEach((track) => track.stop());
    audioRef.current?.pause();
  }, []);

  function mergeExchange(exchange: ChatExchange, temporaryId?: number) {
    setMessages((current) => [
      ...current.filter((message) => message.id !== temporaryId && message.id !== exchange.userMessage.id),
      exchange.userMessage,
      exchange.assistantMessage,
    ].sort((left, right) => left.id - right.id));
  }

  function stopSpeech() {
    const audio = audioRef.current;
    if (!audio) return;
    audio.pause();
    audio.currentTime = 0;
    audioRef.current = null;
    setSpeakingMessageId(null);
  }

  async function playSpeech(message: ChatMessage) {
    if (!message.content.trim()) return;
    stopSpeech();
    setSpeakingMessageId(message.id);
    let audio: HTMLAudioElement | null = null;
    try {
      const result = await synthesizeSpeech(message.content);
      audio = new Audio(`data:${result.contentType};base64,${result.audioBase64}`);
      audioRef.current = audio;
      audio.onended = () => {
        if (audioRef.current === audio) {
          audioRef.current = null;
          setSpeakingMessageId(null);
        }
      };
      audio.onerror = () => {
        if (audioRef.current === audio) {
          audioRef.current = null;
          setSpeakingMessageId(null);
          setNotice("语音播放失败，请重试或检查语音服务设置");
        }
      };
      await audio.play();
    } catch (error) {
      if (audioRef.current === audio) audioRef.current = null;
      setSpeakingMessageId(null);
      setNotice(errorMessage(error));
    }
  }

  async function refreshMessages() {
    const savedMessages = await listMessages();
    setMessages(savedMessages);
  }

  async function discoverScheduleCandidate(message: ChatMessage) {
    try {
      setScheduleCandidate(await getScheduleCandidate(message.content, message.id));
    } catch {
      // A non-schedule message, or an unavailable local parser, must not affect chat.
    }
  }

  async function confirmScheduleCandidate() {
    const candidate = scheduleCandidate;
    if (!candidate || confirmingSchedule) return;
    setConfirmingSchedule(true);
    try {
      await confirmSchedule(candidate);
      setScheduleCandidate(null);
      setNotice("日程已确认并保存到本机");
    } catch (error) {
      setNotice(errorMessage(error));
    } finally {
      setConfirmingSchedule(false);
    }
  }

  async function submitMessage() {
    const content = draft.trim();
    if (!content || sending) return;

    const temporaryId = -Date.now();
    setDraft("");
    setNotice(null);
    setSending(true);
    setMessages((current) => [...current, {
      id: temporaryId,
      role: "user",
      content,
      createdAt: new Date().toISOString(),
      status: "pending",
    }]);

    try {
      const exchange = await sendMessage(content);
      mergeExchange(exchange, temporaryId);
      void discoverScheduleCandidate(exchange.userMessage);
      if (settings.voiceAutoplay) void playSpeech(exchange.assistantMessage);
    } catch (error) {
      setNotice(errorMessage(error));
      try {
        await refreshMessages();
      } catch {
        setMessages((current) => current.map((message) => (
          message.id === temporaryId ? { ...message, status: "failed" } : message
        )));
      }
    } finally {
      setSending(false);
    }
  }

  async function retryFailedMessage(messageId: number) {
    if (sending) return;
    setSending(true);
    setNotice(null);
    setMessages((current) => current.map((message) => (
      message.id === messageId ? { ...message, status: "pending" } : message
    )));
    try {
      const exchange = await retryMessage(messageId);
      mergeExchange(exchange);
      if (settings.voiceAutoplay) void playSpeech(exchange.assistantMessage);
    } catch (error) {
      setNotice(errorMessage(error));
      try {
        await refreshMessages();
      } catch {
        setMessages((current) => current.map((message) => (
          message.id === messageId ? { ...message, status: "failed" } : message
        )));
      }
    } finally {
      setSending(false);
    }
  }

  function handleComposerKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void submitMessage();
    }
  }

  function audioFileExtension(mimeType: string): string {
    if (mimeType.includes("mp4") || mimeType.includes("m4a")) return "m4a";
    if (mimeType.includes("ogg")) return "ogg";
    if (mimeType.includes("wav")) return "wav";
    return "webm";
  }

  function supportedAudioMimeType(mimeType: string): string {
    const normalized = mimeType.split(";", 1)[0]?.trim().toLowerCase();
    return normalized || "audio/webm";
  }

  async function transcribeRecording(blob: Blob) {
    if (blob.size === 0) {
      setNotice("没有录到声音，请再试一次");
      return;
    }
    setTranscribing(true);
    setNotice(null);
    try {
      const audio = Array.from(new Uint8Array(await blob.arrayBuffer()));
      const mimeType = supportedAudioMimeType(blob.type);
      const result = await transcribeAudio(audio, `recording-${Date.now()}.${audioFileExtension(mimeType)}`, mimeType);
      const text = result.text.trim();
      if (!text) throw new Error("没有识别出可确认的文字，请说得更清楚一些");
      setTranscriptDraft(text);
    } catch (error) {
      setNotice(errorMessage(error));
    } finally {
      setTranscribing(false);
    }
  }

  async function startRecording() {
    if (sending || transcribing) return;
    if (!navigator.mediaDevices?.getUserMedia || typeof MediaRecorder === "undefined") {
      setNotice("当前设备不支持录音；请使用文字输入，或在支持麦克风的桌面环境中打开 Nova");
      return;
    }
    setNotice(null);
    setTranscriptDraft(null);
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const preferredMimeType = "audio/webm;codecs=opus";
      const options = MediaRecorder.isTypeSupported(preferredMimeType) ? { mimeType: preferredMimeType } : undefined;
      const recorder = new MediaRecorder(stream, options);
      recordingStreamRef.current = stream;
      recordingChunksRef.current = [];
      recorderRef.current = recorder;
      recorder.ondataavailable = (event) => {
        if (event.data.size > 0) recordingChunksRef.current.push(event.data);
      };
      recorder.onerror = () => setNotice("录音时发生错误，请检查麦克风权限后重试");
      recorder.onstop = () => {
        const mimeType = supportedAudioMimeType(recorder.mimeType || recordingChunksRef.current[0]?.type || "audio/webm");
        const recording = new Blob(recordingChunksRef.current, { type: mimeType });
        stream.getTracks().forEach((track) => track.stop());
        recordingStreamRef.current = null;
        recorderRef.current = null;
        setRecording(false);
        void transcribeRecording(recording);
      };
      recorder.start();
      setRecording(true);
    } catch (error) {
      setNotice(error instanceof DOMException && error.name === "NotAllowedError"
        ? "未获得麦克风权限。请在系统设置中允许 Nova 使用麦克风后重试"
        : errorMessage(error));
    }
  }

  function stopRecording() {
    const recorder = recorderRef.current;
    if (!recorder || recorder.state === "inactive") return;
    recorder.stop();
  }

  function confirmTranscript() {
    const text = transcriptDraft?.trim();
    if (!text) {
      setNotice("识别文字不能为空");
      return;
    }
    setDraft((current) => current.trim() ? `${current.trim()} ${text}` : text);
    setTranscriptDraft(null);
    requestAnimationFrame(() => composerRef.current?.focus());
  }

  return (
    <section className="chat-page">
      <header className="topbar">
        <div>
          <p className="eyebrow">一直都在</p>
          <h1>晚上好，我是 {name}</h1>
        </div>
        <div className="topbar__actions">
          <StatusPill status={status} />
          <WindowModeButton mode={windowMode} onChange={onWindowModeChange} />
        </div>
      </header>

      <div className="conversation" ref={conversationRef}>
        <div className="day-divider"><span>今天</span></div>
        {loadingHistory ? (
          <p className="conversation__loading">正在读取聊天记录…</p>
        ) : messages.length === 0 ? (
          <article className="message message--assistant">
            <div className="avatar">{name.slice(0, 1)}</div>
            <div>
              <p className="message__name">{name}</p>
              <div className="bubble">
                <p>嗨，我已经准备好了。想从今天发生的哪件小事聊起？</p>
              </div>
            </div>
          </article>
        ) : messages.map((message) => {
          const isAssistant = message.role === "assistant";
          const isFailed = message.status === "failed";
          return (
            <article className={isAssistant ? "message message--assistant" : "message message--user"} key={message.id}>
              {isAssistant && <div className="avatar">{name.slice(0, 1)}</div>}
              <div className="message__content">
                {isAssistant && <p className="message__name">{name}</p>}
                <div className="bubble">
                  <p>{message.content}</p>
                </div>
                {isAssistant && message.status === "sent" && (
                  <button
                    aria-label={speakingMessageId === message.id ? "停止朗读" : "朗读这条回复"}
                    className={speakingMessageId === message.id ? "speech-button speech-button--playing" : "speech-button"}
                    onClick={() => speakingMessageId === message.id ? stopSpeech() : void playSpeech(message)}
                    type="button"
                  >
                    {speakingMessageId === message.id ? "■ 停止朗读" : "♬ 朗读"}
                  </button>
                )}
                {message.status !== "sent" && (
                  <div className={isFailed ? "message__state message__state--failed" : "message__state"}>
                    <span>{isFailed ? "发送失败，消息已保留在本机" : "正在发送…"}</span>
                    {isFailed && (
                      <span className="message__recovery">
                        <button disabled={sending} onClick={() => void retryFailedMessage(message.id)} type="button">重试</button>
                        <button onClick={onOpenSettings} type="button">检查 API 设置</button>
                      </span>
                    )}
                  </div>
                )}
              </div>
            </article>
          );
        })}
      </div>

      {scheduleCandidate && (
        <section className="transcript-confirmation schedule-confirmation" aria-label="确认日程">
          <div><p className="eyebrow">待确认日程</p><strong>{scheduleCandidate.title}</strong><p>{displayDateTime(scheduleCandidate.scheduledAt)} 提醒</p></div>
          <div className="transcript-confirmation__actions">
            <button className="secondary-button" disabled={confirmingSchedule} onClick={() => setScheduleCandidate(null)} type="button">暂不创建</button>
            <button className="primary-button" disabled={confirmingSchedule} onClick={() => void confirmScheduleCandidate()} type="button">{confirmingSchedule ? "保存中…" : "确认日程"}</button>
          </div>
        </section>
      )}

      {transcriptDraft !== null && (
        <section className="transcript-confirmation" aria-label="确认语音识别结果">
          <div><p className="eyebrow">语音识别结果</p><strong>确认后会填入输入框，你仍可修改后再发送。</strong></div>
          <textarea aria-label="编辑识别文字" autoFocus onChange={(event) => setTranscriptDraft(event.target.value)} rows={3} value={transcriptDraft} />
          <div className="transcript-confirmation__actions">
            <button className="secondary-button" onClick={() => setTranscriptDraft(null)} type="button">取消</button>
            <button className="primary-button" onClick={confirmTranscript} type="button">使用这段文字</button>
          </div>
        </section>
      )}

      <footer className="composer">
        <button
          aria-label={recording ? "停止录音" : "开始录音"}
          aria-pressed={recording}
          className={recording ? "voice-button voice-button--recording" : "voice-button"}
          disabled={sending || transcribing}
          onClick={() => recording ? stopRecording() : void startRecording()}
          type="button"
        >{recording ? "■" : "◉"}</button>
        <textarea
          aria-label="输入消息"
          disabled={sending}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={handleComposerKeyDown}
          placeholder={`和${name}说点什么……`}
          ref={composerRef}
          rows={1}
          value={draft}
        />
        <button className="send-button" disabled={!draft.trim() || sending} onClick={() => void submitMessage()} type="button">
          {sending ? "发送中…" : "发送"}
        </button>
        <p className={notice ? "composer__hint composer__hint--error" : "composer__hint"} role={notice ? "alert" : undefined}>
          {notice ?? (recording ? "正在录音，点击麦克风结束" : transcribing ? "正在识别语音…" : "按 Enter 发送，Shift + Enter 换行")}
        </p>
      </footer>
    </section>
  );
}

function Toggle({ checked, label, onChange }: {
  checked: boolean;
  label: string;
  onChange: (checked: boolean) => void;
}) {
  return (
    <button
      aria-label={label}
      aria-pressed={checked}
      className={checked ? "toggle toggle--active" : "toggle"}
      onClick={() => onChange(!checked)}
      type="button"
    >
      <span />
    </button>
  );
}

function ApiSettingsCard({ chatOnly = false, onChatReadyChange }: {
  chatOnly?: boolean;
  onChatReadyChange?: (ready: boolean) => void;
}) {
  const capabilities = chatOnly ? (["chat"] as ApiCapability[]) : Object.keys(apiCapabilityMeta) as ApiCapability[];
  const [selected, setSelected] = useState<ApiCapability>("chat");
  const [profiles, setProfiles] = useState<Record<ApiCapability, ApiProfileInput>>({
    chat: emptyApiProfile("chat"),
    transcription: emptyApiProfile("transcription"),
    speech: emptyApiProfile("speech"),
  });
  const [savedStatuses, setSavedStatuses] = useState<Partial<Record<ApiCapability, ApiProfileStatus>>>({});
  const [notice, setNotice] = useState("尚未配置");
  const [busy, setBusy] = useState<"save" | "test" | null>(null);
  const profile = profiles[selected];

  useEffect(() => {
    let active = true;
    Promise.all(capabilities.map(async (capability) => [capability, await getApiProfileStatus(capability)] as const))
      .then((entries) => {
        if (!active) return;
        const nextProfiles = { ...profiles };
        const nextStatuses: Partial<Record<ApiCapability, ApiProfileStatus>> = {};
        for (const [capability, status] of entries) {
          if (!status) continue;
          nextStatuses[capability] = status;
          nextProfiles[capability] = { ...status, apiKey: null };
        }
        setProfiles(nextProfiles);
        setSavedStatuses(nextStatuses);
        onChatReadyChange?.(nextStatuses.chat?.connectionTested ?? false);
        if (nextStatuses.chat) setNotice("已读取本机配置");
      })
      .catch(() => {
        // Browser-only preview has no native credential store.
      });
    return () => { active = false; };
    // Profiles are intentionally initialized only once from native storage.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function update(changes: Partial<ApiProfileInput>) {
    setProfiles({ ...profiles, [selected]: { ...profile, ...changes } });
  }

  async function save() {
    setBusy("save");
    setNotice("正在保存…");
    try {
      const saved = await saveApiProfile(profile);
      setSavedStatuses({ ...savedStatuses, [selected]: saved });
      setProfiles({ ...profiles, [selected]: { ...saved, apiKey: null } });
      if (selected === "chat") onChatReadyChange?.(false);
      setNotice("配置已保存，API Key 已进入系统凭据存储");
    } catch (error) {
      setNotice(errorMessage(error));
    } finally {
      setBusy(null);
    }
  }

  async function test() {
    setBusy("test");
    setNotice("正在测试连接…");
    try {
      const result = await testApiProfile(selected);
      const saved = savedStatuses[selected];
      if (saved) {
        setSavedStatuses({ ...savedStatuses, [selected]: { ...saved, connectionTested: true } });
      }
      if (selected === "chat") onChatReadyChange?.(true);
      setNotice(`连接正常 · ${result.latencyMs} ms`);
    } catch (error) {
      setNotice(errorMessage(error));
    } finally {
      setBusy(null);
    }
  }

  return (
    <section className="settings-card api-card">
      <span className="section-tag">AI 服务</span>
      <h2>OpenAI 兼容接口</h2>
      <div className="capability-tabs">
        {capabilities.map((capability) => (
          <button className={selected === capability ? "is-active" : ""} key={capability} onClick={() => {
            setSelected(capability);
            setNotice(savedStatuses[capability] ? "已读取本机配置" : "尚未配置");
          }} type="button">
            {apiCapabilityMeta[capability].label}
          </button>
        ))}
      </div>
      <label>
        <span>接口地址</span>
        <input value={profile.baseUrl} onChange={(event) => update({ baseUrl: event.target.value })} />
      </label>
      <label>
        <span>接口路径</span>
        <input value={profile.path} onChange={(event) => update({ path: event.target.value })} />
      </label>
      <label>
        <span>模型</span>
        <input value={profile.model} onChange={(event) => update({ model: event.target.value })} />
      </label>
      <label>
        <span>API Key</span>
        <input
          autoComplete="off"
          placeholder={savedStatuses[selected]?.hasApiKey ? "已安全保存；留空表示不修改" : "请输入 API Key"}
          type="password"
          value={profile.apiKey ?? ""}
          onChange={(event) => update({ apiKey: event.target.value || null })}
        />
      </label>
      <div className="api-actions">
        <span className={notice.startsWith("连接正常") ? "api-notice api-notice--ready" : "api-notice"}>{notice}</span>
        <div>
          <button className="secondary-button" disabled={busy !== null} onClick={() => void save()} type="button">
            {busy === "save" ? "保存中…" : "保存配置"}
          </button>
          <button className="primary-button" disabled={busy !== null || !savedStatuses[selected]} onClick={() => void test()} type="button">
            {busy === "test" ? "测试中…" : "测试连接"}
          </button>
        </div>
      </div>
    </section>
  );
}

function OnboardingPage({ persona, onPersonaSaved, onComplete }: {
  persona: PersonaProfile | null;
  onPersonaSaved: (persona: PersonaProfile) => void;
  onComplete: () => void;
}) {
  const [draft, setDraft] = useState(persona ?? defaultPersona);
  const [personaReady, setPersonaReady] = useState(Boolean(persona));
  const [chatReady, setChatReady] = useState(false);
  const [notice, setNotice] = useState(persona ? "角色设定已保存" : "先让她认识你一点");
  const [saving, setSaving] = useState(false);

  async function savePersonaStep() {
    setSaving(true);
    setNotice("正在保存角色设定…");
    try {
      const saved = await savePersona(draft);
      onPersonaSaved(saved);
      setDraft(saved);
      setPersonaReady(true);
      setNotice("角色设定已保存");
    } catch (error) {
      setNotice(errorMessage(error));
    } finally {
      setSaving(false);
    }
  }

  return (
    <section className="onboarding-page">
      <header className="onboarding-heading">
        <div>
          <p className="eyebrow">欢迎来到 Nova</p>
          <h1>让她先认识你一点</h1>
          <p>完成角色设定并测试对话 API，之后就能开始聊天。</p>
        </div>
        <div className="step-indicator" aria-label="配置进度">
          <span className={personaReady ? "is-done" : "is-active"}>1</span>
          <i />
          <span className={chatReady ? "is-done" : personaReady ? "is-active" : ""}>2</span>
          <i />
          <span className={personaReady && chatReady ? "is-active" : ""}>3</span>
        </div>
      </header>

      <div className="onboarding-grid">
        <section className="settings-card onboarding-persona">
          <span className="section-tag">第一步 · 角色设定</span>
          <h2>她会怎么陪伴你？</h2>
          <label><span>名字</span><input maxLength={32} value={draft.name} onChange={(event) => {
            setDraft({ ...draft, name: event.target.value }); setPersonaReady(false);
          }} /></label>
          <label><span>性格</span><textarea maxLength={240} rows={3} value={draft.personality} onChange={(event) => {
            setDraft({ ...draft, personality: event.target.value }); setPersonaReady(false);
          }} /></label>
          <label><span>说话方式</span><textarea maxLength={240} rows={3} value={draft.speechStyle} onChange={(event) => {
            setDraft({ ...draft, speechStyle: event.target.value }); setPersonaReady(false);
          }} /></label>
          <div className="onboarding-action">
            <span>{notice}</span>
            <button className="primary-button" disabled={saving} onClick={() => void savePersonaStep()} type="button">
              {saving ? "保存中…" : personaReady ? "重新保存" : "保存角色设定"}
            </button>
          </div>
        </section>

        <ApiSettingsCard chatOnly onChatReadyChange={setChatReady} />

        <section className="onboarding-finish">
          <div>
            <span className="section-tag">第三步 · 准备聊天</span>
            <h2>从一句话开始</h2>
            <p>{personaReady && chatReady ? `${draft.name} 已经准备好见你了。` : "完成前两步后，就可以进入长期聊天时间线。"}</p>
          </div>
          <button className="primary-button" disabled={!personaReady || !chatReady} onClick={onComplete} type="button">进入聊天 →</button>
        </section>
      </div>
    </section>
  );
}

function SettingsPage({ settings, persona, onSettingsSaved, onPersonaSaved }: {
  settings: AppSettings;
  persona: PersonaProfile | null;
  onSettingsSaved: (settings: AppSettings) => void;
  onPersonaSaved: (persona: PersonaProfile) => void;
}) {
  const [settingsDraft, setSettingsDraft] = useState(settings);
  const [personaDraft, setPersonaDraft] = useState(persona ?? defaultPersona);
  const [notice, setNotice] = useState("所有设置保存在本机");
  const [saving, setSaving] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [testingNotification, setTestingNotification] = useState(false);

  useEffect(() => setSettingsDraft(settings), [settings]);
  useEffect(() => setPersonaDraft(persona ?? defaultPersona), [persona]);

  async function submit(event: FormEvent) {
    event.preventDefault();
    setSaving(true);
    setNotice("正在保存…");
    try {
      const [savedPersona, savedSettings] = await Promise.all([
        savePersona(personaDraft),
        saveSettings(settingsDraft),
      ]);
      onPersonaSaved(savedPersona);
      onSettingsSaved(savedSettings);
      setNotice("已保存");
    } catch (error) {
      setNotice(errorMessage(error));
    } finally {
      setSaving(false);
    }
  }

  async function downloadExport() {
    setExporting(true);
    setNotice("正在准备导出…");
    try {
      const json = await exportLocalData();
      const url = URL.createObjectURL(new Blob([json], { type: "application/json;charset=utf-8" }));
      const link = document.createElement("a");
      link.href = url;
      link.download = `nova-data-${new Date().toISOString().slice(0, 10)}.json`;
      link.click();
      URL.revokeObjectURL(url);
      setNotice("聊天与记忆已导出；文件不包含 API Key");
    } catch (error) {
      setNotice(errorMessage(error));
    } finally {
      setExporting(false);
    }
  }

  async function testNotification() {
    setTestingNotification(true);
    setNotice("正在发送测试通知…");
    try {
      await showNotification("Nova", "通知已经准备好了；日程提醒和主动陪伴会在合适的时候出现在这里。");
      setNotice("测试通知已发送");
    } catch (error) {
      setNotice(errorMessage(error));
    } finally {
      setTestingNotification(false);
    }
  }

  return (
    <section className="settings-page">
      <header className="page-heading">
        <div>
          <p className="eyebrow">陪伴设置</p>
          <h1>设置</h1>
          <p>调整她的性格、主题和陪伴方式。</p>
        </div>
        <span className={notice === "已保存" ? "save-state save-state--done" : "save-state"}>{notice}</span>
      </header>

      <form className="settings-grid" onSubmit={submit}>
        <div className="settings-column">
          <section className="settings-card">
            <span className="section-tag">角色设定</span>
            <h2>她会怎么陪伴你？</h2>
            <label>
              <span>名字</span>
              <input maxLength={32} value={personaDraft.name} onChange={(event) => setPersonaDraft({ ...personaDraft, name: event.target.value })} />
            </label>
            <label>
              <span>性格</span>
              <textarea maxLength={240} rows={3} value={personaDraft.personality} onChange={(event) => setPersonaDraft({ ...personaDraft, personality: event.target.value })} />
            </label>
            <label>
              <span>说话方式</span>
              <textarea maxLength={240} rows={3} value={personaDraft.speechStyle} onChange={(event) => setPersonaDraft({ ...personaDraft, speechStyle: event.target.value })} />
            </label>
          </section>

          <ApiSettingsCard />
        </div>

        <div className="settings-column">
          <section className="settings-card">
            <span className="section-tag">主题外观</span>
            <h2>选择主题色</h2>
            <div className="theme-picker" aria-label="主题色">
              {(["rose", "lavender", "mint", "blue", "peach"] as const).map((theme) => (
                <button
                  aria-label={theme}
                  className={settingsDraft.theme === theme ? `theme-dot theme-dot--${theme} is-selected` : `theme-dot theme-dot--${theme}`}
                  key={theme}
                  onClick={() => setSettingsDraft({ ...settingsDraft, theme })}
                  type="button"
                />
              ))}
            </div>
            <div className="setting-row">
              <div><strong>深色主题</strong><small>降低夜间使用时的亮度</small></div>
              <Toggle checked={settingsDraft.darkMode} label="深色主题" onChange={(darkMode) => setSettingsDraft({ ...settingsDraft, darkMode })} />
            </div>
          </section>

          <section className="settings-card">
            <span className="section-tag">主动陪伴</span>
            <div className="setting-row">
              <div><strong>桌面提示</strong><small>允许她在合适的时候主动找你</small></div>
              <Toggle checked={settingsDraft.proactiveEnabled} label="桌面提示" onChange={(proactiveEnabled) => setSettingsDraft({ ...settingsDraft, proactiveEnabled })} />
            </div>
            <div className="time-row">
              <div><strong>免打扰时间</strong><small>仅暂停主动陪伴，不影响日程提醒</small></div>
              <div className="time-inputs">
                <input aria-label="免打扰开始时间" type="time" value={settingsDraft.dndStart ?? ""} onChange={(event) => setSettingsDraft({ ...settingsDraft, dndStart: event.target.value || null })} />
                <span>至</span>
                <input aria-label="免打扰结束时间" type="time" value={settingsDraft.dndEnd ?? ""} onChange={(event) => setSettingsDraft({ ...settingsDraft, dndEnd: event.target.value || null })} />
              </div>
            </div>
            <div className="setting-row">
              <div><strong>语音自动播放</strong><small>收到回复时自动播放语音</small></div>
              <Toggle checked={settingsDraft.voiceAutoplay} label="语音自动播放" onChange={(voiceAutoplay) => setSettingsDraft({ ...settingsDraft, voiceAutoplay })} />
            </div>
            <button className="secondary-button" disabled={testingNotification} onClick={() => void testNotification()} type="button">
              {testingNotification ? "发送中…" : "发送测试通知"}
            </button>
          </section>

          <section className="settings-card">
            <span className="section-tag">本地数据</span>
            <h2>导出聊天与记忆</h2>
            <p>生成可读的 JSON 文件，仅包含聊天记录和记忆，不包含 API Key、服务配置或其他凭据。</p>
            <button className="secondary-button" disabled={exporting} onClick={() => void downloadExport()} type="button">
              {exporting ? "导出中…" : "导出本地数据"}
            </button>
          </section>

          <button className="primary-button" disabled={saving} type="submit">{saving ? "保存中…" : "保存设置"}</button>
        </div>
      </form>
    </section>
  );
}

export function App() {
  const [page, setPage] = useState<Page>("chat");
  const [status, setStatus] = useState<BootstrapResponse | null>(null);
  const [settings, setSettings] = useState<AppSettings>(defaultSettings);
  const [persona, setPersona] = useState<PersonaProfile | null>(null);
  const [windowMode, setWindowModeState] = useState<WindowMode>("management");
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    let active = true;
    Promise.all([bootstrap(), getSettings(), getPersona()])
      .then(([bootstrapStatus, savedSettings, savedPersona]) => {
        if (!active) return;
        setStatus(bootstrapStatus);
        setWindowModeState(bootstrapStatus.windowMode);
        setSettings(savedSettings);
        setPersona(savedPersona);
        setLoaded(true);
      })
      .catch(() => {
        // Browser-only Vite preview has no Tauri IPC bridge.
        if (active) setLoaded(true);
      });
    return () => { active = false; };
  }, []);

  useEffect(() => {
    document.documentElement.dataset.theme = settings.theme;
    document.documentElement.dataset.colorScheme = settings.darkMode ? "dark" : "light";
  }, [settings.darkMode, settings.theme]);

  useEffect(() => {
    if (!settings.proactiveEnabled) return;
    const activityKey = "nova:last-activity-at";
    const dispatchKey = "nova:last-proactive-at";
    const markActive = () => localStorage.setItem(activityKey, String(Date.now()));
    const inQuietHours = () => {
      if (!settings.dndStart || !settings.dndEnd) return false;
      const parse = (value: string) => Number(value.slice(0, 2)) * 60 + Number(value.slice(3, 5));
      const [start, end] = [parse(settings.dndStart), parse(settings.dndEnd)];
      const now = new Date(); const minute = now.getHours() * 60 + now.getMinutes();
      return start === end || (start < end ? minute >= start && minute < end : minute >= start || minute < end);
    };
    async function checkIn() {
      const now = Date.now();
      const lastActive = Number(localStorage.getItem(activityKey) ?? now);
      const lastDispatch = Number(localStorage.getItem(dispatchKey) ?? 0);
      if (inQuietHours() || now - lastActive < 7_200_000 || now - lastDispatch < 21_600_000) return;
      try { await showNotification("Nova", "你忙完了吗？记得喝口水，我一直都在。"); localStorage.setItem(dispatchKey, String(now)); } catch { /* retry next tick */ }
    }
    if (!localStorage.getItem(activityKey)) markActive();
    const events: Array<keyof DocumentEventMap> = ["pointerdown", "keydown", "touchstart"];
    events.forEach((event) => document.addEventListener(event, markActive, { passive: true }));
    const timer = window.setInterval(() => void checkIn(), 60_000);
    return () => { events.forEach((event) => document.removeEventListener(event, markActive)); window.clearInterval(timer); };
  }, [settings.dndEnd, settings.dndStart, settings.proactiveEnabled]);

  useEffect(() => {
    let active = true;
    const dispatchedPrefix = "nova:reminder-dispatched:";
    async function dispatchDueReminders() {
      try {
        const now = Date.now();
        const schedules = await listSchedules();
        for (const schedule of schedules) {
          if (!active || schedule.status !== "scheduled") continue;
          const remindAt = new Date(schedule.remindAt).getTime();
          if (!Number.isFinite(remindAt) || remindAt > now) continue;
          const key = `${dispatchedPrefix}${schedule.id}:${schedule.remindAt}`;
          if (localStorage.getItem(key)) continue;
          await showNotification("Nova 日程提醒", `${schedule.title} · ${displayDateTime(schedule.scheduledAt)}`);
          localStorage.setItem(key, new Date().toISOString());
        }
      } catch {
        // A failed notification is intentionally not marked dispatched, so a later tick or
        // application restart can retry without losing the reminder.
      }
    }
    void dispatchDueReminders();
    const timer = window.setInterval(() => void dispatchDueReminders(), 60_000);
    return () => { active = false; window.clearInterval(timer); };
  }, []);

  async function toggleWindowMode() {
    const nextMode: WindowMode = windowMode === "compact" ? "management" : "compact";
    try {
      await setWindowMode(nextMode);
      setWindowModeState(nextMode);
      if (nextMode === "compact") setPage("chat");
    } catch {
      // Keep the current mode if native window resizing is unavailable.
    }
  }

  if (!loaded) {
    return <main className="loading-screen"><span className="brand__mark">N</span><p>正在准备你的陪伴空间…</p></main>;
  }

  if (status && !status.onboardingComplete) {
    return (
      <OnboardingPage
        onComplete={() => setStatus({ ...status, onboardingComplete: true })}
        onPersonaSaved={setPersona}
        persona={persona}
      />
    );
  }

  return (
    <main className={windowMode === "compact" ? "app-shell app-shell--compact" : "app-shell"}>
      <aside className="sidebar">
        <div className="brand">
          <span className="brand__mark">N</span>
          <div><strong>Nova</strong><small>你的长期陪伴</small></div>
        </div>

        <nav aria-label="主导航">
          {navigation.map((item) => (
            <button className={page === item.id ? "nav-item nav-item--active" : "nav-item"} key={item.id} onClick={() => setPage(item.id)} type="button">
              <span>{item.glyph}</span>{item.label}
            </button>
          ))}
        </nav>

        <div className="sidebar__footer">
          <span className="privacy-mark">⌁</span><p>本地优先</p>
          <small>聊天与设置只保存在这台设备</small>
        </div>
      </aside>

      <div className="workspace">
        {page === "chat" && (
          <ChatPage
            onOpenSettings={() => {
              if (windowMode === "compact") void toggleWindowMode();
              setPage("settings");
            }}
            onWindowModeChange={() => void toggleWindowMode()}
            persona={persona}
            settings={settings}
            status={status}
            windowMode={windowMode}
          />
        )}
        {page === "memory" && <MemoryPage />}
        {page === "schedule" && <SchedulePage />}
        {page === "settings" && (
          <SettingsPage onPersonaSaved={setPersona} onSettingsSaved={setSettings} persona={persona} settings={settings} />
        )}
      </div>
    </main>
  );
}
