import { useEffect, useState, type FormEvent } from "react";
import {
  bootstrap,
  getApiProfileStatus,
  getPersona,
  getSettings,
  saveApiProfile,
  savePersona,
  saveSettings,
  setWindowMode,
  testApiProfile,
  type ApiCapability,
  type ApiProfileInput,
  type ApiProfileStatus,
  type AppSettings,
  type BootstrapResponse,
  type PersonaProfile,
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

function errorMessage(error: unknown): string {
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

function EmptyPage({ page }: { page: "memory" | "schedule" }) {
  const copy = {
    memory: ["记忆", "以后重要的小事，会被好好收在这里。"],
    schedule: ["日程", "通过聊天创建的约定，会按日期出现在这里。"],
  } as const;

  return (
    <section className="empty-page">
      <span className="empty-page__mark">✦</span>
      <h1>{copy[page][0]}</h1>
      <p>{copy[page][1]}</p>
    </section>
  );
}

function ChatPage({
  status,
  persona,
  windowMode,
  onWindowModeChange,
  onOpenSettings,
}: {
  status: BootstrapResponse | null;
  persona: PersonaProfile | null;
  windowMode: WindowMode;
  onWindowModeChange: () => void;
  onOpenSettings: () => void;
}) {
  const name = persona?.name ?? "Nova";
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

      <div className="conversation">
        <div className="day-divider"><span>今天</span></div>
        <article className="message message--assistant">
          <div className="avatar">{name.slice(0, 1)}</div>
          <div>
            <p className="message__name">{name}</p>
            <div className="bubble">
              <p>嗨，我已经准备好了。完成 AI 服务配置后，我们就可以从这里开始慢慢认识彼此。</p>
            </div>
          </div>
        </article>
      </div>

      <footer className="composer">
        <button className="voice-button" type="button" aria-label="按住说话" disabled>◉</button>
        <textarea placeholder={`和${name}说点什么……`} rows={1} disabled />
        <button className="send-button" type="button" onClick={onOpenSettings}>配置 API</button>
        <p className="composer__hint">完成 AI 服务配置后即可开始对话</p>
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
            status={status}
            windowMode={windowMode}
          />
        )}
        {(page === "memory" || page === "schedule") && <EmptyPage page={page} />}
        {page === "settings" && (
          <SettingsPage onPersonaSaved={setPersona} onSettingsSaved={setSettings} persona={persona} settings={settings} />
        )}
      </div>
    </main>
  );
}
