import {
  type ComponentProps,
  type ReactNode,
  useEffect,
  useState,
} from "react";
import { Activity, KeyRound, Mic, Settings2 } from "lucide-react";
import {
  type AppConfig,
  type DesktopOverview,
  getOverview,
  restartDaemon,
  saveConfig,
  saveProviderKey,
  setAutostartEnabled,
  toggleRecording,
} from "./hermes";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
  CardDescription,
  CardFooter,
} from "@/components/ui/card";

type SaveState = "idle" | "saving" | "saved" | "error";

type DraftFields = {
  fillerWords: string;
  wordOverrides: string;
  restHeaders: string;
  restBody: string;
};

type DivProps = ComponentProps<"div">;

function providerLabel(provider: string) {
  switch (provider) {
    case "groq":
      return "Groq";
    case "openai":
      return "OpenAI";
    case "elevenlabs":
      return "ElevenLabs";
    default:
      return provider;
  }
}

function formatJson(value: unknown) {
  return JSON.stringify(value, null, 2);
}

function parseJsonRecord(input: string, label: string) {
  const trimmed = input.trim();
  if (!trimmed) {
    return {};
  }
  const parsed = JSON.parse(trimmed) as unknown;
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new Error(`${label} must be a JSON object.`);
  }
  return parsed as Record<string, unknown>;
}

function normalizeNullableText(value: string) {
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

function parseNumberValue(value: string, fallback: number) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : fallback;
}

function makeDrafts(config: AppConfig): DraftFields {
  return {
    fillerWords: config.filler_words.join(", "),
    wordOverrides: formatJson(config.word_overrides),
    restHeaders: formatJson(config.rest_headers),
    restBody: formatJson(config.rest_body),
  };
}

function Field(props: {
  id: string;
  label: string;
  description?: string;
  children: ReactNode;
}) {
  return (
    <div className="space-y-1.5">
      <Label htmlFor={props.id}>{props.label}</Label>
      {props.children}
      {props.description ? (
        <p className="text-xs text-muted-foreground">{props.description}</p>
      ) : null}
    </div>
  );
}

function ToggleField(props: {
  id: string;
  label: string;
  description: string;
  checked: boolean;
  onCheckedChange: (value: boolean) => void;
}) {
  return (
    <div className="flex items-start justify-between gap-3 rounded-md border p-2.5">
      <div className="space-y-1">
        <Label htmlFor={props.id}>{props.label}</Label>
        <p className="text-xs text-muted-foreground">{props.description}</p>
      </div>
      <Switch
        id={props.id}
        checked={props.checked}
        onCheckedChange={props.onCheckedChange}
      />
    </div>
  );
}

export function App() {
  const [overview, setOverview] = useState<DesktopOverview | null>(null);
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [drafts, setDrafts] = useState<DraftFields>({
    fillerWords: "",
    wordOverrides: "{}",
    restHeaders: "{}",
    restBody: "{}",
  });
  const [saveState, setSaveState] = useState<SaveState>("idle");
  const [statusLine, setStatusLine] = useState("Loading Hermes desktop...");
  const [providerKeyDrafts, setProviderKeyDrafts] = useState<
    Record<string, string>
  >({
    groq: "",
    openai: "",
    elevenlabs: "",
  });

  useEffect(() => {
    void refresh();
  }, []);

  function applyOverview(next: DesktopOverview) {
    setOverview(next);
    setConfig(next.config);
    setDrafts(makeDrafts(next.config));
  }

  async function refresh() {
    const next = await getOverview();
    applyOverview(next);
    setStatusLine(
      next.recording ? "Hermes is actively listening." : "Hermes is ready.",
    );
  }

  function patchConfig(patch: Partial<AppConfig>) {
    if (!config) return;
    setConfig({ ...config, ...patch });
    setSaveState("idle");
  }

  function buildConfigForSave() {
    if (!config) return null;
    return {
      ...config,
      filler_words: drafts.fillerWords
        .split(",")
        .map((word) => word.trim())
        .filter(Boolean),
      word_overrides: parseJsonRecord(
        drafts.wordOverrides,
        "Word overrides",
      ) as Record<string, string>,
      rest_headers: parseJsonRecord(
        drafts.restHeaders,
        "REST headers",
      ) as Record<string, string>,
      rest_body: parseJsonRecord(drafts.restBody, "REST body"),
    };
  }

  async function handleSaveConfig() {
    if (!config) return;
    setSaveState("saving");
    try {
      const nextConfig = buildConfigForSave();
      if (!nextConfig) return;
      const next = await saveConfig(nextConfig);
      applyOverview(next);
      setSaveState("saved");
      setStatusLine("Settings saved and Hermes restarted.");
    } catch (error) {
      console.error(error);
      setSaveState("error");
      setStatusLine(
        error instanceof Error ? error.message : "Could not save config.",
      );
    }
  }

  async function handleToggleRecording() {
    try {
      const next = await toggleRecording();
      applyOverview(next);
      setStatusLine(
        next.recording ? "Dictation started." : "Dictation stopped.",
      );
    } catch (error) {
      console.error(error);
      setStatusLine("Could not toggle dictation.");
    }
  }

  async function handleRestart() {
    try {
      const next = await restartDaemon();
      applyOverview(next);
      setStatusLine("Hermes engine restarted.");
    } catch (error) {
      console.error(error);
      setStatusLine("Could not restart engine.");
    }
  }

  async function handleAutostartToggle(enabled: boolean) {
    try {
      const next = await setAutostartEnabled(enabled);
      applyOverview(next);
      setStatusLine(
        enabled ? "Launch at login enabled." : "Launch at login disabled.",
      );
    } catch (error) {
      console.error(error);
      setStatusLine("Could not update launch-at-login.");
    }
  }

  async function handleSaveProviderKey(provider: string) {
    const key = providerKeyDrafts[provider] ?? "";
    if (!key.trim()) {
      setStatusLine(`No ${providerLabel(provider)} key entered.`);
      return;
    }
    try {
      const next = await saveProviderKey(provider, key.trim());
      applyOverview(next);
      setProviderKeyDrafts((current) => ({ ...current, [provider]: "" }));
      setStatusLine(`${providerLabel(provider)} key saved.`);
    } catch (error) {
      console.error(error);
      setStatusLine(`Could not save ${providerLabel(provider)} key.`);
    }
  }

  if (!overview || !config) {
    return (
      <main className="flex min-h-screen items-center justify-center p-4">
        <Card className="w-full max-w-md">
          <CardHeader>
            <CardTitle className="text-base">Hermes Desktop</CardTitle>
            <CardDescription>{statusLine}</CardDescription>
          </CardHeader>
        </Card>
      </main>
    );
  }

  return (
    <main className="min-h-screen p-2">
      <div className="mx-auto max-w-7xl space-y-3">
        <Card>
          <CardContent className="flex flex-col gap-3 p-3 md:flex-row md:items-center md:justify-between">
            <div className="space-y-1">
              <p className="text-xs uppercase tracking-wide text-muted-foreground">
                Hermes Desktop
              </p>
              <h1 className="text-2xl font-semibold tracking-tight">
                Settings
              </h1>
              <p className="text-sm text-muted-foreground">{statusLine}</p>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <Badge
                variant={overview.recording ? "default" : "secondary"}
                className="gap-1.5"
              >
                <Activity className="size-3.5" />
                {overview.recording ? "Recording" : "Idle"}
              </Badge>
              <Button variant="outline" onClick={() => void refresh()}>
                Refresh
              </Button>
              <Button variant="outline" onClick={() => void handleRestart()}>
                Restart Engine
              </Button>
              <Button onClick={() => void handleToggleRecording()}>
                <Mic className="size-4" />
                {overview.recording ? "Stop Dictation" : "Start Dictation"}
              </Button>
              <Button
                onClick={() => void handleSaveConfig()}
                disabled={saveState === "saving"}
              >
                {saveState === "saving" ? "Saving..." : "Save Settings"}
              </Button>
            </div>
          </CardContent>
        </Card>

        <div className="grid gap-2 md:grid-cols-4">
          <Card>
            <CardContent className="p-3">
              <p className="text-xs text-muted-foreground">Engine</p>
              <p className="mt-1 text-sm font-medium">
                {overview.daemonRunning ? "Online" : "Offline"}
              </p>
            </CardContent>
          </Card>
          <Card>
            <CardContent className="p-3">
              <p className="text-xs text-muted-foreground">Primary shortcut</p>
              <p className="mt-1 text-sm font-medium">
                {config.primary_shortcut}
              </p>
            </CardContent>
          </Card>
          <Card>
            <CardContent className="p-3">
              <p className="text-xs text-muted-foreground">Backend</p>
              <p className="mt-1 text-sm font-medium">
                {config.transcription_backend}
              </p>
            </CardContent>
          </Card>
          <Card>
            <CardContent className="p-3">
              <p className="text-xs text-muted-foreground">Config path</p>
              <p className="mt-1 truncate text-sm font-medium text-muted-foreground">
                {overview.configPath}
              </p>
            </CardContent>
          </Card>
        </div>

        <Card>
          <CardContent>
            <Tabs defaultValue="general">
              <TabsList>
                <TabsTrigger value="general">General</TabsTrigger>
                <TabsTrigger value="audio">Audio</TabsTrigger>
                <TabsTrigger value="providers">Providers</TabsTrigger>
                <TabsTrigger value="realtime">Realtime</TabsTrigger>
                <TabsTrigger value="models">Models</TabsTrigger>
                <TabsTrigger value="text">Text Rules</TabsTrigger>
              </TabsList>

              <TabsContent value="general">
                <Card>
                  <CardHeader>
                    <CardTitle className="flex items-center gap-2 text-base">
                      <Settings2 className="size-4" /> Core behavior
                    </CardTitle>
                    <CardDescription>
                      Hotkeys, recording mode, and session behavior.
                    </CardDescription>
                  </CardHeader>
                  <CardContent>
                    <div className="grid gap-3 md:grid-cols-2">
                      <Field id="primary_shortcut" label="Primary shortcut">
                        <Input
                          id="primary_shortcut"
                          value={config.primary_shortcut}
                          onChange={(event) =>
                            patchConfig({
                              primary_shortcut: event.target.value,
                            })
                          }
                        />
                      </Field>
                      <Field id="secondary_shortcut" label="Secondary shortcut">
                        <Input
                          id="secondary_shortcut"
                          value={config.secondary_shortcut ?? ""}
                          onChange={(event) =>
                            patchConfig({
                              secondary_shortcut: normalizeNullableText(
                                event.target.value,
                              ),
                            })
                          }
                        />
                      </Field>
                      <Field id="secondary_language" label="Secondary language">
                        <Input
                          id="secondary_language"
                          value={config.secondary_language ?? ""}
                          onChange={(event) =>
                            patchConfig({
                              secondary_language: normalizeNullableText(
                                event.target.value,
                              ),
                            })
                          }
                        />
                      </Field>
                      <Field id="cancel_shortcut" label="Cancel shortcut">
                        <Input
                          id="cancel_shortcut"
                          value={config.cancel_shortcut ?? ""}
                          onChange={(event) =>
                            patchConfig({
                              cancel_shortcut: normalizeNullableText(
                                event.target.value,
                              ),
                            })
                          }
                        />
                      </Field>
                      <Field
                        id="long_form_submit_shortcut"
                        label="Long-form submit shortcut"
                      >
                        <Input
                          id="long_form_submit_shortcut"
                          value={config.long_form_submit_shortcut ?? ""}
                          onChange={(event) =>
                            patchConfig({
                              long_form_submit_shortcut: normalizeNullableText(
                                event.target.value,
                              ),
                            })
                          }
                        />
                      </Field>
                      <Field id="recording_mode" label="Recording mode">
                        <Select
                          value={config.recording_mode}
                          onValueChange={(value) =>
                            patchConfig({ recording_mode: value })
                          }
                        >
                          <SelectTrigger id="recording_mode">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="toggle">Toggle</SelectItem>
                            <SelectItem value="push_to_talk">
                              Push to talk
                            </SelectItem>
                            <SelectItem value="long_form">Long form</SelectItem>
                          </SelectContent>
                        </Select>
                      </Field>
                      <Field id="language" label="Language">
                        <Input
                          id="language"
                          value={config.language ?? ""}
                          onChange={(event) =>
                            patchConfig({
                              language: normalizeNullableText(
                                event.target.value,
                              ),
                            })
                          }
                        />
                      </Field>
                      <Field id="threads" label="Worker threads">
                        <Input
                          id="threads"
                          type="number"
                          min={1}
                          value={config.threads}
                          onChange={(event) =>
                            patchConfig({
                              threads: parseNumberValue(
                                event.target.value,
                                config.threads,
                              ),
                            })
                          }
                        />
                      </Field>

                      <ToggleField
                        id="autostart"
                        label="Launch at login"
                        description="Start Hermes with your desktop session."
                        checked={overview.autostartEnabled}
                        onCheckedChange={(value) =>
                          void handleAutostartToggle(value)
                        }
                      />
                      <ToggleField
                        id="use_hypr_bindings"
                        label="Use Hypr bindings"
                        description="Prefer compositor-provided bindings."
                        checked={config.use_hypr_bindings}
                        onCheckedChange={(value) =>
                          patchConfig({ use_hypr_bindings: value })
                        }
                      />
                      <ToggleField
                        id="show_overlay"
                        label="Show overlay"
                        description="Display the live pill while dictating."
                        checked={config.show_overlay}
                        onCheckedChange={(value) =>
                          patchConfig({ show_overlay: value })
                        }
                      />
                      <ToggleField
                        id="auto_submit"
                        label="Auto submit"
                        description="Insert result as soon as transcription completes."
                        checked={config.auto_submit}
                        onCheckedChange={(value) =>
                          patchConfig({ auto_submit: value })
                        }
                      />
                    </div>
                  </CardContent>
                </Card>
              </TabsContent>

              <TabsContent value="audio">
                <Card>
                  <CardHeader>
                    <CardTitle className="text-base">
                      Microphone & injection
                    </CardTitle>
                    <CardDescription>
                      Device routing, paste behavior, and clipboard options.
                    </CardDescription>
                  </CardHeader>
                  <CardContent>
                    <div className="grid gap-3 md:grid-cols-2">
                      <Field id="audio_device_id" label="Input device">
                        <Select
                          value={String(
                            config.audio_device_id ??
                              overview.devices[0]?.id ??
                              0,
                          )}
                          onValueChange={(value) => {
                            const id = Number(value);
                            const device = overview.devices.find(
                              (item) => item.id === id,
                            );
                            patchConfig({
                              audio_device_id: id,
                              audio_device_name: device?.name ?? null,
                            });
                          }}
                        >
                          <SelectTrigger id="audio_device_id">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            {overview.devices.map((device) => (
                              <SelectItem
                                key={device.id}
                                value={String(device.id)}
                              >
                                {device.name}
                                {device.is_default ? " (default)" : ""}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      </Field>
                      <Field
                        id="selected_device_name"
                        label="Selected device name"
                      >
                        <Input
                          id="selected_device_name"
                          value={config.selected_device_name ?? ""}
                          onChange={(event) =>
                            patchConfig({
                              selected_device_name: normalizeNullableText(
                                event.target.value,
                              ),
                            })
                          }
                        />
                      </Field>
                      <Field id="audio_device_name" label="Audio device name">
                        <Input
                          id="audio_device_name"
                          value={config.audio_device_name ?? ""}
                          onChange={(event) =>
                            patchConfig({
                              audio_device_name: normalizeNullableText(
                                event.target.value,
                              ),
                            })
                          }
                        />
                      </Field>
                      <Field id="paste_mode" label="Paste mode">
                        <Select
                          value={config.paste_mode}
                          onValueChange={(value) =>
                            patchConfig({ paste_mode: value })
                          }
                        >
                          <SelectTrigger id="paste_mode">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="ctrl_shift">
                              Ctrl+Shift+V
                            </SelectItem>
                            <SelectItem value="ctrl">Ctrl+V</SelectItem>
                            <SelectItem value="alt">Alt+V</SelectItem>
                            <SelectItem value="super">Super+V</SelectItem>
                          </SelectContent>
                        </Select>
                      </Field>
                      <Field
                        id="clipboard_clear_delay"
                        label="Clipboard clear delay"
                      >
                        <Input
                          id="clipboard_clear_delay"
                          type="number"
                          step="0.1"
                          value={config.clipboard_clear_delay}
                          onChange={(event) =>
                            patchConfig({
                              clipboard_clear_delay: parseNumberValue(
                                event.target.value,
                                config.clipboard_clear_delay,
                              ),
                            })
                          }
                        />
                      </Field>
                      <Field id="whisper_prompt" label="Whisper prompt">
                        <Textarea
                          id="whisper_prompt"
                          rows={4}
                          value={config.whisper_prompt}
                          onChange={(event) =>
                            patchConfig({
                              whisper_prompt: event.target.value,
                            })
                          }
                        />
                      </Field>
                      <ToggleField
                        id="clipboard_behavior"
                        label="Clipboard behavior"
                        description="Enable clipboard management while injecting text."
                        checked={config.clipboard_behavior}
                        onCheckedChange={(value) =>
                          patchConfig({ clipboard_behavior: value })
                        }
                      />
                    </div>
                  </CardContent>
                </Card>
              </TabsContent>

              <TabsContent value="providers">
                <Card>
                  <CardHeader>
                    <CardTitle className="text-base">
                      Backend and providers
                    </CardTitle>
                    <CardDescription>
                      Configure remote provider behavior and API keys.
                    </CardDescription>
                  </CardHeader>
                  <CardContent className="space-y-2">
                    <div className="grid gap-3 md:grid-cols-2">
                      <Field
                        id="transcription_backend"
                        label="Transcription backend"
                      >
                        <Select
                          value={config.transcription_backend}
                          onValueChange={(value) =>
                            patchConfig({ transcription_backend: value })
                          }
                        >
                          <SelectTrigger id="transcription_backend">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="rest-api">REST API</SelectItem>
                            <SelectItem value="realtime-ws">
                              Realtime WebSocket
                            </SelectItem>
                            <SelectItem value="whisper-rs">
                              Whisper.rs
                            </SelectItem>
                            <SelectItem value="faster-whisper">
                              Faster Whisper
                            </SelectItem>
                          </SelectContent>
                        </Select>
                      </Field>
                      <Field id="rest_api_provider" label="REST provider">
                        <Select
                          value={config.rest_api_provider ?? "groq"}
                          onValueChange={(value) =>
                            patchConfig({
                              rest_api_provider: normalizeNullableText(value),
                            })
                          }
                        >
                          <SelectTrigger id="rest_api_provider">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="groq">Groq</SelectItem>
                            <SelectItem value="openai">OpenAI</SelectItem>
                            <SelectItem value="elevenlabs">
                              ElevenLabs
                            </SelectItem>
                          </SelectContent>
                        </Select>
                      </Field>
                      <Field id="rest_endpoint_url" label="REST endpoint URL">
                        <Input
                          id="rest_endpoint_url"
                          value={config.rest_endpoint_url ?? ""}
                          onChange={(event) =>
                            patchConfig({
                              rest_endpoint_url: normalizeNullableText(
                                event.target.value,
                              ),
                            })
                          }
                        />
                      </Field>
                      <Field id="rest_timeout" label="REST timeout (seconds)">
                        <Input
                          id="rest_timeout"
                          type="number"
                          min={1}
                          value={config.rest_timeout}
                          onChange={(event) =>
                            patchConfig({
                              rest_timeout: parseNumberValue(
                                event.target.value,
                                config.rest_timeout,
                              ),
                            })
                          }
                        />
                      </Field>
                    </div>
                    <div className="grid gap-2 md:grid-cols-3">
                      {Object.entries(overview.providerKeys).map(
                        ([providerName, present]) => (
                          <Card key={providerName}>
                            <CardHeader className="space-y-1 pb-2">
                              <CardTitle className="flex items-center justify-between text-sm">
                                <span className="flex items-center gap-2">
                                  <KeyRound className="size-4" />
                                  {providerLabel(providerName)}
                                </span>
                                <Badge
                                  variant={present ? "default" : "secondary"}
                                >
                                  {present ? "Stored" : "Missing"}
                                </Badge>
                              </CardTitle>
                              <CardDescription className="text-xs">
                                Saved in OS keychain.
                              </CardDescription>
                            </CardHeader>
                            <CardContent className="space-y-2">
                              <Input
                                type="password"
                                placeholder={`Paste ${providerLabel(providerName)} key`}
                                value={providerKeyDrafts[providerName] ?? ""}
                                onChange={(event) =>
                                  setProviderKeyDrafts((current) => ({
                                    ...current,
                                    [providerName]: event.target.value,
                                  }))
                                }
                              />
                              <Button
                                variant="outline"
                                size="sm"
                                className="w-full"
                                onClick={() =>
                                  void handleSaveProviderKey(providerName)
                                }
                              >
                                Save key
                              </Button>
                            </CardContent>
                          </Card>
                        ),
                      )}
                    </div>
                  </CardContent>
                </Card>
              </TabsContent>

              <TabsContent value="realtime">
                <Card>
                  <CardHeader>
                    <CardTitle className="text-base">
                      Realtime and remote payloads
                    </CardTitle>
                    <CardDescription>
                      WebSocket controls and raw REST headers/body payloads.
                    </CardDescription>
                  </CardHeader>
                  <CardContent>
                    <div className="grid gap-3 md:grid-cols-2">
                      <Field id="websocket_provider" label="WebSocket provider">
                        <Input
                          id="websocket_provider"
                          value={config.websocket_provider ?? ""}
                          onChange={(event) =>
                            patchConfig({
                              websocket_provider: normalizeNullableText(
                                event.target.value,
                              ),
                            })
                          }
                        />
                      </Field>
                      <Field id="websocket_model" label="WebSocket model">
                        <Input
                          id="websocket_model"
                          value={config.websocket_model ?? ""}
                          onChange={(event) =>
                            patchConfig({
                              websocket_model: normalizeNullableText(
                                event.target.value,
                              ),
                            })
                          }
                        />
                      </Field>
                      <Field id="websocket_url" label="WebSocket URL">
                        <Input
                          id="websocket_url"
                          value={config.websocket_url ?? ""}
                          onChange={(event) =>
                            patchConfig({
                              websocket_url: normalizeNullableText(
                                event.target.value,
                              ),
                            })
                          }
                        />
                      </Field>
                      <Field id="realtime_mode" label="Realtime mode">
                        <Input
                          id="realtime_mode"
                          value={config.realtime_mode}
                          onChange={(event) =>
                            patchConfig({ realtime_mode: event.target.value })
                          }
                        />
                      </Field>
                      <Field
                        id="realtime_timeout"
                        label="Realtime timeout (seconds)"
                      >
                        <Input
                          id="realtime_timeout"
                          type="number"
                          min={1}
                          value={config.realtime_timeout}
                          onChange={(event) =>
                            patchConfig({
                              realtime_timeout: parseNumberValue(
                                event.target.value,
                                config.realtime_timeout,
                              ),
                            })
                          }
                        />
                      </Field>
                      <Field
                        id="realtime_buffer_max_seconds"
                        label="Realtime buffer max seconds"
                      >
                        <Input
                          id="realtime_buffer_max_seconds"
                          type="number"
                          min={1}
                          value={config.realtime_buffer_max_seconds}
                          onChange={(event) =>
                            patchConfig({
                              realtime_buffer_max_seconds: parseNumberValue(
                                event.target.value,
                                config.realtime_buffer_max_seconds,
                              ),
                            })
                          }
                        />
                      </Field>
                      <Field id="rest_headers" label="REST headers JSON">
                        <Textarea
                          id="rest_headers"
                          rows={7}
                          value={drafts.restHeaders}
                          onChange={(event) =>
                            setDrafts((current) => ({
                              ...current,
                              restHeaders: event.target.value,
                            }))
                          }
                        />
                      </Field>
                      <Field id="rest_body" label="REST body JSON">
                        <Textarea
                          id="rest_body"
                          rows={7}
                          value={drafts.restBody}
                          onChange={(event) =>
                            setDrafts((current) => ({
                              ...current,
                              restBody: event.target.value,
                            }))
                          }
                        />
                      </Field>
                    </div>
                  </CardContent>
                </Card>
              </TabsContent>

              <TabsContent value="models">
                <Card>
                  <CardHeader>
                    <CardTitle className="text-base">
                      Local model backends
                    </CardTitle>
                    <CardDescription>
                      Whisper, ONNX and Faster Whisper configuration.
                    </CardDescription>
                  </CardHeader>
                  <CardContent className="space-y-2">
                    <div className="grid gap-3 md:grid-cols-2">
                      <Field id="model" label="Whisper model">
                        <Input
                          id="model"
                          value={config.model}
                          onChange={(event) =>
                            patchConfig({ model: event.target.value })
                          }
                        />
                      </Field>
                      <Field id="model_path" label="Model path">
                        <Input
                          id="model_path"
                          value={config.model_path ?? ""}
                          onChange={(event) =>
                            patchConfig({
                              model_path: normalizeNullableText(
                                event.target.value,
                              ),
                            })
                          }
                        />
                      </Field>
                      <Field id="onnx_asr_model" label="ONNX model">
                        <Input
                          id="onnx_asr_model"
                          value={config.onnx_asr_model}
                          onChange={(event) =>
                            patchConfig({
                              onnx_asr_model: event.target.value,
                            })
                          }
                        />
                      </Field>
                      <Field
                        id="onnx_asr_quantization"
                        label="ONNX quantization"
                      >
                        <Input
                          id="onnx_asr_quantization"
                          value={config.onnx_asr_quantization ?? ""}
                          onChange={(event) =>
                            patchConfig({
                              onnx_asr_quantization: normalizeNullableText(
                                event.target.value,
                              ),
                            })
                          }
                        />
                      </Field>
                      <Field
                        id="faster_whisper_model"
                        label="Faster Whisper model"
                      >
                        <Input
                          id="faster_whisper_model"
                          value={config.faster_whisper_model}
                          onChange={(event) =>
                            patchConfig({
                              faster_whisper_model: event.target.value,
                            })
                          }
                        />
                      </Field>
                      <Field
                        id="faster_whisper_device"
                        label="Faster Whisper device"
                      >
                        <Input
                          id="faster_whisper_device"
                          value={config.faster_whisper_device}
                          onChange={(event) =>
                            patchConfig({
                              faster_whisper_device: event.target.value,
                            })
                          }
                        />
                      </Field>
                      <Field
                        id="faster_whisper_compute_type"
                        label="Faster Whisper compute type"
                      >
                        <Input
                          id="faster_whisper_compute_type"
                          value={config.faster_whisper_compute_type}
                          onChange={(event) =>
                            patchConfig({
                              faster_whisper_compute_type: event.target.value,
                            })
                          }
                        />
                      </Field>
                    </div>
                    <div className="grid gap-2 md:grid-cols-2">
                      <ToggleField
                        id="onnx_asr_use_vad"
                        label="ONNX VAD"
                        description="Enable voice activity detection for ONNX ASR."
                        checked={config.onnx_asr_use_vad}
                        onCheckedChange={(value) =>
                          patchConfig({ onnx_asr_use_vad: value })
                        }
                      />
                      <ToggleField
                        id="faster_whisper_vad_filter"
                        label="Faster Whisper VAD filter"
                        description="Apply VAD filtering for Faster Whisper."
                        checked={config.faster_whisper_vad_filter}
                        onCheckedChange={(value) =>
                          patchConfig({ faster_whisper_vad_filter: value })
                        }
                      />
                    </div>
                  </CardContent>
                </Card>
              </TabsContent>

              <TabsContent value="text">
                <Card>
                  <CardHeader>
                    <CardTitle className="text-base">
                      Text processing and long-form
                    </CardTitle>
                    <CardDescription>
                      Output cleanup rules and long-form buffering limits.
                    </CardDescription>
                  </CardHeader>
                  <CardContent>
                    <div className="grid gap-3 md:grid-cols-2">
                      <Field
                        id="long_form_temp_limit_mb"
                        label="Long-form temp limit (MB)"
                      >
                        <Input
                          id="long_form_temp_limit_mb"
                          type="number"
                          min={1}
                          value={config.long_form_temp_limit_mb}
                          onChange={(event) =>
                            patchConfig({
                              long_form_temp_limit_mb: parseNumberValue(
                                event.target.value,
                                config.long_form_temp_limit_mb,
                              ),
                            })
                          }
                        />
                      </Field>
                      <Field
                        id="long_form_auto_save_interval"
                        label="Long-form auto-save interval (seconds)"
                      >
                        <Input
                          id="long_form_auto_save_interval"
                          type="number"
                          min={1}
                          value={config.long_form_auto_save_interval}
                          onChange={(event) =>
                            patchConfig({
                              long_form_auto_save_interval: parseNumberValue(
                                event.target.value,
                                config.long_form_auto_save_interval,
                              ),
                            })
                          }
                        />
                      </Field>
                      <Field
                        id="filler_words"
                        label="Filler words"
                        description="Comma-separated list."
                      >
                        <Textarea
                          id="filler_words"
                          rows={5}
                          value={drafts.fillerWords}
                          onChange={(event) =>
                            setDrafts((current) => ({
                              ...current,
                              fillerWords: event.target.value,
                            }))
                          }
                        />
                      </Field>
                      <Field id="word_overrides" label="Word overrides JSON">
                        <Textarea
                          id="word_overrides"
                          rows={5}
                          value={drafts.wordOverrides}
                          onChange={(event) =>
                            setDrafts((current) => ({
                              ...current,
                              wordOverrides: event.target.value,
                            }))
                          }
                        />
                      </Field>
                      <ToggleField
                        id="filter_filler_words"
                        label="Filter filler words"
                        description="Remove fillers like 'uh' and 'um' from output."
                        checked={config.filter_filler_words}
                        onCheckedChange={(value) =>
                          patchConfig({ filter_filler_words: value })
                        }
                      />
                      <ToggleField
                        id="symbol_replacements"
                        label="Symbol replacements"
                        description="Interpret words like 'comma' and 'period' as punctuation."
                        checked={config.symbol_replacements}
                        onCheckedChange={(value) =>
                          patchConfig({ symbol_replacements: value })
                        }
                      />
                    </div>
                  </CardContent>
                </Card>
              </TabsContent>
            </Tabs>
          </CardContent>
        </Card>
      </div>
    </main>
  );
}
