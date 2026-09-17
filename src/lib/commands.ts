import { invoke } from "@tauri-apps/api/core";

export type AppErrorCategory =
  | "configuration"
  | "network"
  | "authorization"
  | "service"
  | "audio"
  | "database"
  | "platformPermission"
  | "internal";

export interface AppError {
  category: AppErrorCategory;
  code: string;
  message: string;
  retryable: boolean;
}

export interface BootstrapResponse {
  appVersion: string;
  onboardingComplete: boolean;
  databaseReady: boolean;
  windowMode: WindowMode;
}

export interface PersonaProfile {
  name: string;
  personality: string;
  speechStyle: string;
}

export interface AppSettings {
  theme: "rose" | "lavender" | "mint" | "blue" | "peach";
  darkMode: boolean;
  dndStart: string | null;
  dndEnd: string | null;
  voiceAutoplay: boolean;
  proactiveEnabled: boolean;
}

export type WindowMode = "compact" | "management";
export type ApiCapability = "chat" | "transcription" | "speech";

export interface ApiProfileInput {
  capability: ApiCapability;
  baseUrl: string;
  path: string;
  model: string;
  apiKey: string | null;
  enabled: boolean;
}

export interface ApiProfileStatus {
  capability: ApiCapability;
  baseUrl: string;
  path: string;
  model: string;
  hasApiKey: boolean;
  enabled: boolean;
  connectionTested: boolean;
}

export interface ApiTestResult {
  success: boolean;
  latencyMs: number;
}

export type ChatRole = "user" | "assistant";
export type ChatMessageStatus = "pending" | "sent" | "failed";

export interface ChatMessage {
  id: number;
  role: ChatRole;
  content: string;
  createdAt: string;
  status: ChatMessageStatus;
}

export interface ChatExchange {
  userMessage: ChatMessage;
  assistantMessage: ChatMessage;
}

export interface TranscriptionResult {
  text: string;
}

export interface SpeechSynthesisResult {
  audioBase64: string;
  contentType: string;
}

export interface MemoryRecord {
  id: number;
  content: string;
  sourceMessageId: number;
  createdAt: string;
  updatedAt: string;
}

export type ScheduleStatus = "scheduled" | "completed" | "cancelled" | string;

export interface ScheduleRecord {
  id: number;
  title: string;
  scheduledAt: string;
  remindAt: string;
  sourceMessageId: number | null;
  status: ScheduleStatus;
}

export interface ConfirmScheduleInput {
  title: string;
  scheduledAt: string;
  remindAt: string;
  sourceMessageId?: number;
}

export interface UpdateScheduleInput {
  id: number;
  title: string;
  scheduledAt: string;
  remindAt: string;
  status: ScheduleStatus;
}

export interface ScheduleCandidate {
  title: string;
  scheduledAt: string;
  remindAt: string;
  sourceMessageId: number;
}

export async function bootstrap(): Promise<BootstrapResponse> {
  return invoke<BootstrapResponse>("bootstrap");
}

export async function getPersona(): Promise<PersonaProfile | null> {
  return invoke<PersonaProfile | null>("get_persona");
}

export async function savePersona(persona: PersonaProfile): Promise<PersonaProfile> {
  return invoke<PersonaProfile>("save_persona", { persona });
}

export async function getSettings(): Promise<AppSettings> {
  return invoke<AppSettings>("get_settings");
}

export async function saveSettings(settings: AppSettings): Promise<AppSettings> {
  return invoke<AppSettings>("save_settings", { settings });
}

export async function setWindowMode(mode: WindowMode): Promise<void> {
  return invoke<void>("set_window_mode", { mode });
}

export async function getApiProfileStatus(
  capability: ApiCapability,
): Promise<ApiProfileStatus | null> {
  return invoke<ApiProfileStatus | null>("get_api_profile_status", { capability });
}

export async function saveApiProfile(profile: ApiProfileInput): Promise<ApiProfileStatus> {
  return invoke<ApiProfileStatus>("save_api_profile", { profile });
}

export async function testApiProfile(capability: ApiCapability): Promise<ApiTestResult> {
  return invoke<ApiTestResult>("test_api_profile", { capability });
}

export async function listMessages(): Promise<ChatMessage[]> {
  return invoke<ChatMessage[]>("list_messages");
}

export async function sendMessage(content: string): Promise<ChatExchange> {
  return invoke<ChatExchange>("send_message", { content });
}

export async function retryMessage(messageId: number): Promise<ChatExchange> {
  return invoke<ChatExchange>("retry_message", { messageId });
}

export async function transcribeAudio(
  audio: number[],
  fileName?: string,
  mimeType?: string,
): Promise<TranscriptionResult> {
  return invoke<TranscriptionResult>("transcribe_audio", { audio, fileName, mimeType });
}

export async function synthesizeSpeech(
  text: string,
  voice?: string,
): Promise<SpeechSynthesisResult> {
  return invoke<SpeechSynthesisResult>("synthesize_speech", { text, voice });
}

export async function listMemories(): Promise<MemoryRecord[]> {
  return invoke<MemoryRecord[]>("list_memories");
}

export async function updateMemory(id: number, content: string): Promise<MemoryRecord | null> {
  return invoke<MemoryRecord | null>("update_memory", { id, content });
}

export async function deleteMemory(id: number): Promise<boolean> {
  return invoke<boolean>("delete_memory", { id });
}

export async function listSchedules(): Promise<ScheduleRecord[]> {
  return invoke<ScheduleRecord[]>("list_schedules");
}

export async function confirmSchedule(input: ConfirmScheduleInput): Promise<ScheduleRecord> {
  return invoke<ScheduleRecord>("confirm_schedule", { input });
}

export async function updateSchedule(input: UpdateScheduleInput): Promise<ScheduleRecord | null> {
  return invoke<ScheduleRecord | null>("update_schedule", { input });
}

export async function deleteSchedule(id: number): Promise<boolean> {
  return invoke<boolean>("delete_schedule", { id });
}

export async function getScheduleCandidate(content: string, sourceMessageId: number): Promise<ScheduleCandidate | null> {
  const today = new Date();
  return invoke<ScheduleCandidate | null>("get_schedule_candidate", {
    input: {
      content,
      sourceMessageId,
      year: today.getFullYear(),
      month: today.getMonth() + 1,
      day: today.getDate(),
    },
  });
}

export async function exportLocalData(): Promise<string> {
  return invoke<string>("export_local_data");
}

export async function showNotification(title: string, body: string): Promise<void> {
  return invoke<void>("show_notification", { title, body });
}
