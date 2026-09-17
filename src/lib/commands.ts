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
