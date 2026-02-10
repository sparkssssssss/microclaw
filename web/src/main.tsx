import React, { useEffect, useMemo, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import {
  Badge,
  Box,
  Button,
  Card,
  Dialog,
  Flex,
  Heading,
  ScrollArea,
  Separator,
  Text,
  TextArea,
  TextField,
  Theme,
} from '@radix-ui/themes'
import '@radix-ui/themes/styles.css'

type SessionItem = { session_key: string }
type MessageItem = {
  id: string
  sender_name: string
  content: string
  is_from_bot: boolean
  timestamp: string
}

type ConfigPayload = Record<string, unknown>

type StreamHandlers = {
  onStatus: (payload: Record<string, unknown>) => void
  onToolStart: (payload: Record<string, unknown>) => void
  onToolResult: (payload: Record<string, unknown>) => void
  onDelta: (payload: Record<string, unknown>) => void
  onDone: (payload: Record<string, unknown>) => void
  onError: (payload: Record<string, unknown>) => void
}

function readToken(): string {
  return localStorage.getItem('microclaw_web_token') || ''
}

function saveToken(token: string): void {
  localStorage.setItem('microclaw_web_token', token)
}

function makeHeaders(token: string, options: RequestInit = {}): HeadersInit {
  const headers: Record<string, string> = {
    ...(options.headers as Record<string, string> | undefined),
  }
  if (options.body && !headers['Content-Type']) {
    headers['Content-Type'] = 'application/json'
  }
  if (token.trim()) {
    headers.Authorization = `Bearer ${token.trim()}`
  }
  return headers
}

async function api<T>(
  path: string,
  token: string,
  options: RequestInit = {},
): Promise<T> {
  const res = await fetch(path, { ...options, headers: makeHeaders(token, options) })
  const data = (await res.json().catch(() => ({}))) as Record<string, unknown>
  if (!res.ok) {
    throw new Error(String(data.error || data.message || `HTTP ${res.status}`))
  }
  return data as T
}

async function streamRun(
  runId: string,
  token: string,
  signal: AbortSignal,
  handlers: StreamHandlers,
): Promise<void> {
  const query = new URLSearchParams({ run_id: runId })
  const res = await fetch(`/api/stream?${query.toString()}`, {
    method: 'GET',
    headers: makeHeaders(token),
    signal,
    cache: 'no-store',
  })

  if (!res.ok) {
    const errText = await res.text().catch(() => '')
    throw new Error(errText || `HTTP ${res.status}`)
  }
  if (!res.body) {
    throw new Error('empty stream body')
  }

  const reader = res.body.getReader()
  const decoder = new TextDecoder()
  let pending = ''
  let eventName = 'message'
  let dataLines: string[] = []

  const dispatch = () => {
    if (dataLines.length === 0) return
    const data = dataLines.join('\n')
    dataLines = []

    let payload: Record<string, unknown> = {}
    try {
      payload = JSON.parse(data) as Record<string, unknown>
    } catch {
      payload = { raw: data }
    }

    switch (eventName) {
      case 'status':
        handlers.onStatus(payload)
        break
      case 'tool_start':
        handlers.onToolStart(payload)
        break
      case 'tool_result':
        handlers.onToolResult(payload)
        break
      case 'delta':
        handlers.onDelta(payload)
        break
      case 'done':
        handlers.onDone(payload)
        break
      case 'error':
        handlers.onError(payload)
        break
      default:
        break
    }
    eventName = 'message'
  }

  const handleLine = (line: string) => {
    if (line === '') {
      dispatch()
      return
    }
    if (line.startsWith(':')) return
    const sep = line.indexOf(':')
    const field = sep >= 0 ? line.slice(0, sep) : line
    let value = sep >= 0 ? line.slice(sep + 1) : ''
    if (value.startsWith(' ')) value = value.slice(1)
    if (field === 'event') eventName = value
    if (field === 'data') dataLines.push(value)
  }

  while (true) {
    const { done, value } = await reader.read()
    pending += decoder.decode(value || new Uint8Array(), { stream: !done })

    while (true) {
      const idx = pending.indexOf('\n')
      if (idx < 0) break
      let line = pending.slice(0, idx)
      pending = pending.slice(idx + 1)
      if (line.endsWith('\r')) line = line.slice(0, -1)
      handleLine(line)
    }

    if (done) {
      if (pending.length > 0) {
        let line = pending
        if (line.endsWith('\r')) line = line.slice(0, -1)
        handleLine(line)
      }
      dispatch()
      break
    }
  }
}

function App() {
  const [token, setToken] = useState<string>(readToken())
  const [sessions, setSessions] = useState<SessionItem[]>([])
  const [sessionKey, setSessionKey] = useState<string>('main')
  const [messages, setMessages] = useState<MessageItem[]>([])
  const [messageInput, setMessageInput] = useState<string>('')
  const [senderName, setSenderName] = useState<string>('web-user')
  const [error, setError] = useState<string>('')
  const [statusText, setStatusText] = useState<string>('')
  const [sending, setSending] = useState<boolean>(false)
  const [configOpen, setConfigOpen] = useState<boolean>(false)
  const [config, setConfig] = useState<ConfigPayload | null>(null)
  const [configDraft, setConfigDraft] = useState<Record<string, unknown>>({})
  const [saveStatus, setSaveStatus] = useState<string>('')
  const streamAbortRef = useRef<AbortController | null>(null)

  const canSend = useMemo(() => messageInput.trim().length > 0 && !sending, [messageInput, sending])

  async function loadSessions(): Promise<void> {
    const data = await api<{ sessions?: SessionItem[] }>('/api/sessions', token)
    setSessions(Array.isArray(data.sessions) ? data.sessions : [])
  }

  async function loadHistory(target = sessionKey): Promise<void> {
    const query = new URLSearchParams({ session_key: target, limit: '200' })
    const data = await api<{ messages?: MessageItem[] }>(`/api/history?${query.toString()}`, token)
    setMessages(Array.isArray(data.messages) ? data.messages : [])
  }

  function closeStream(): void {
    if (streamAbortRef.current) {
      streamAbortRef.current.abort()
      streamAbortRef.current = null
    }
  }

  function addOptimisticUserMessage(content: string): void {
    const msg: MessageItem = {
      id: `u-${Date.now()}`,
      sender_name: senderName || 'web-user',
      content,
      is_from_bot: false,
      timestamp: new Date().toISOString(),
    }
    setMessages((prev) => [...prev, msg])
  }

  function ensureStreamingAssistant(): void {
    setMessages((prev) => {
      const id = 'streaming-assistant'
      if (prev.some((m) => m.id === id)) return prev
      return [
        ...prev,
        {
          id,
          sender_name: 'assistant',
          content: '',
          is_from_bot: true,
          timestamp: new Date().toISOString(),
        },
      ]
    })
  }

  function appendAssistantDelta(delta: string): void {
    setMessages((prev) =>
      prev.map((m) => (m.id === 'streaming-assistant' ? { ...m, content: (m.content || '') + delta } : m)),
    )
  }

  async function onSend(): Promise<void> {
    const trimmed = messageInput.trim()
    if (!trimmed || sending) return

    setSending(true)
    setError('')
    setStatusText('Sending...')
    closeStream()

    try {
      addOptimisticUserMessage(trimmed)
      ensureStreamingAssistant()
      setMessageInput('')

      const sendRes = await api<{ run_id?: string }>('/api/send_stream', token, {
        method: 'POST',
        body: JSON.stringify({ session_key: sessionKey, sender_name: senderName, message: trimmed }),
      })

      const runId = sendRes.run_id
      if (!runId) throw new Error('missing run_id')

      const aborter = new AbortController()
      streamAbortRef.current = aborter

      void streamRun(runId, token, aborter.signal, {
        onStatus: (data) => {
          const message = typeof data.message === 'string' ? data.message : ''
          if (message) setStatusText(message)
        },
        onToolStart: (data) => {
          const name = typeof data.name === 'string' ? data.name : ''
          if (name) setStatusText(`tool: ${name}...`)
        },
        onToolResult: (data) => {
          const name = typeof data.name === 'string' ? data.name : ''
          if (!name) return
          const isError = Boolean(data.is_error)
          const durationMs = typeof data.duration_ms === 'number' ? data.duration_ms : 0
          const bytes = typeof data.bytes === 'number' ? data.bytes : 0
          const suffix = isError ? 'error' : 'ok'
          setStatusText(`tool: ${name} (${suffix}) ${durationMs}ms ${bytes}b`)
        },
        onDelta: (data) => {
          const delta = typeof data.delta === 'string' ? data.delta : ''
          if (delta) appendAssistantDelta(delta)
        },
        onDone: async (data) => {
          const response = typeof data.response === 'string' ? data.response : ''
          setMessages((prev) =>
            prev.map((m) => (m.id === 'streaming-assistant' ? { ...m, content: response } : m)),
          )
          closeStream()
          setSending(false)
          setStatusText('Done')
          await Promise.all([loadSessions(), loadHistory(sessionKey)])
        },
        onError: (data) => {
          const msg = typeof data.error === 'string' ? data.error : 'Stream error'
          setError(msg)
          closeStream()
          setSending(false)
        },
      }).catch((e) => {
        if (aborter.signal.aborted) return
        setError(e instanceof Error ? e.message : String(e))
        closeStream()
        setSending(false)
      })
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
      setSending(false)
      setStatusText('')
      closeStream()
      await loadHistory(sessionKey).catch(() => {})
    }
  }

  async function onResetSession(): Promise<void> {
    try {
      await api('/api/reset', token, {
        method: 'POST',
        body: JSON.stringify({ session_key: sessionKey }),
      })
      await loadHistory(sessionKey)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  async function openConfig(): Promise<void> {
    setSaveStatus('')
    const data = await api<{ config?: ConfigPayload }>('/api/config', token)
    setConfig(data.config || null)
    setConfigDraft({
      llm_provider: data.config?.llm_provider || '',
      model: data.config?.model || '',
      api_key: '',
      max_tokens: Number(data.config?.max_tokens ?? 8192),
      max_tool_iterations: Number(data.config?.max_tool_iterations ?? 100),
      show_thinking: Boolean(data.config?.show_thinking),
      web_enabled: Boolean(data.config?.web_enabled),
      web_host: String(data.config?.web_host || '127.0.0.1'),
      web_port: Number(data.config?.web_port ?? 3900),
      web_auth_token: '',
    })
    setConfigOpen(true)
  }

  async function saveConfigChanges(): Promise<void> {
    try {
      const payload: Record<string, unknown> = {
        llm_provider: String(configDraft.llm_provider || ''),
        model: String(configDraft.model || ''),
        max_tokens: Number(configDraft.max_tokens || 8192),
        max_tool_iterations: Number(configDraft.max_tool_iterations || 100),
        show_thinking: Boolean(configDraft.show_thinking),
        web_enabled: Boolean(configDraft.web_enabled),
        web_host: String(configDraft.web_host || '127.0.0.1'),
        web_port: Number(configDraft.web_port || 3900),
      }
      const apiKey = String(configDraft.api_key || '').trim()
      const webAuth = String(configDraft.web_auth_token || '').trim()
      if (apiKey) payload.api_key = apiKey
      if (webAuth) payload.web_auth_token = webAuth

      await api('/api/config', token, { method: 'PUT', body: JSON.stringify(payload) })
      setSaveStatus('Saved. Restart microclaw to apply changes.')
    } catch (e) {
      setSaveStatus(`Save failed: ${e instanceof Error ? e.message : String(e)}`)
    }
  }

  useEffect(() => {
    saveToken(token)
  }, [token])

  useEffect(() => {
    ;(async () => {
      try {
        setError('')
        await Promise.all([loadSessions(), loadHistory(sessionKey)])
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e))
      }
    })()
    return closeStream
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  useEffect(() => {
    loadHistory(sessionKey).catch((e) => setError(e instanceof Error ? e.message : String(e)))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionKey])

  return (
    <Theme appearance="light" accentColor="teal" grayColor="slate" radius="medium" scaling="100%">
      <Flex style={{ height: '100%', padding: 16, gap: 16 }}>
        <Card style={{ width: 280, display: 'flex', flexDirection: 'column', gap: 12 }}>
          <Flex justify="between" align="center">
            <Heading size="4">MicroClaw</Heading>
            <Button
              size="1"
              variant="soft"
              onClick={async () => {
                try {
                  await openConfig()
                } catch (e) {
                  setError(e instanceof Error ? e.message : String(e))
                }
              }}
            >
              Config
            </Button>
          </Flex>
          <Text size="2" color="gray">
            Local sessions
          </Text>
          <TextField.Root value={token} onChange={(e) => setToken(e.target.value)} placeholder="Bearer token (optional)" />
          <Separator />
          <ScrollArea type="auto" style={{ height: '100%' }}>
            <Flex direction="column" gap="2">
              <Button variant={sessionKey === 'main' ? 'solid' : 'soft'} onClick={() => setSessionKey('main')}>
                main
              </Button>
              {sessions.map((s) => (
                <Button
                  key={s.session_key}
                  variant={sessionKey === s.session_key ? 'solid' : 'soft'}
                  onClick={() => setSessionKey(s.session_key)}
                  style={{ justifyContent: 'flex-start' }}
                >
                  {s.session_key}
                </Button>
              ))}
            </Flex>
          </ScrollArea>
        </Card>

        <Card style={{ flex: 1, display: 'flex', flexDirection: 'column', gap: 12, minWidth: 0 }}>
          <Flex justify="between" align="center">
            <Flex align="center" gap="2">
              <Heading size="4">{sessionKey}</Heading>
              <Badge color="teal" variant="soft">
                SSE
              </Badge>
            </Flex>
            <Flex gap="2">
              <Button
                size="1"
                variant="soft"
                onClick={() => loadHistory(sessionKey).catch((e) => setError(e instanceof Error ? e.message : String(e)))}
              >
                Refresh
              </Button>
              <Button size="1" variant="soft" color="orange" onClick={onResetSession}>
                Reset Session
              </Button>
            </Flex>
          </Flex>

          {statusText ? (
            <Text size="2" color="gray">
              {statusText}
            </Text>
          ) : null}
          {error ? <Text color="red" size="2">{error}</Text> : null}

          <Box style={{ flex: 1, minHeight: 0 }}>
            <ScrollArea
              type="auto"
              style={{ height: '100%', border: '1px solid #e2e8f0', borderRadius: 8, padding: 10, background: '#ffffff' }}
            >
              <Flex direction="column" gap="2">
                {messages.map((m) => (
                  <Card key={m.id} style={{ background: m.is_from_bot ? '#f0fdfa' : '#f8fafc' }}>
                    <Flex justify="between" align="center">
                      <Text weight="bold" size="2">
                        {m.sender_name}
                      </Text>
                      <Text size="1" color="gray">
                        {new Date(m.timestamp).toLocaleString()}
                      </Text>
                    </Flex>
                    <Text as="p" size="2" style={{ whiteSpace: 'pre-wrap', marginTop: 6 }}>
                      {m.content}
                    </Text>
                  </Card>
                ))}
              </Flex>
            </ScrollArea>
          </Box>

          <Flex gap="2">
            <TextField.Root style={{ width: 180 }} value={senderName} onChange={(e) => setSenderName(e.target.value)} placeholder="sender name" />
            <TextArea
              value={messageInput}
              onChange={(e) => setMessageInput(e.target.value)}
              placeholder="Type message..."
              style={{ flex: 1, minHeight: 84 }}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
                  e.preventDefault()
                  void onSend()
                }
              }}
            />
            <Button disabled={!canSend} onClick={() => void onSend()}>
              {sending ? 'Streaming...' : 'Send'}
            </Button>
          </Flex>
          <Text size="1" color="gray">
            Tip: Ctrl/Cmd + Enter to send
          </Text>
        </Card>

        <Dialog.Root open={configOpen} onOpenChange={setConfigOpen}>
          <Dialog.Content maxWidth="640px">
            <Dialog.Title>Runtime Config</Dialog.Title>
            <Dialog.Description size="2" mb="3">
              Save writes to microclaw.config.yaml. Restart is required.
            </Dialog.Description>
            {config ? (
              <Flex direction="column" gap="2">
                <Text size="2" color="gray">Current provider: {String(config.llm_provider || '')}</Text>
                <TextField.Root value={String(configDraft.llm_provider || '')} onChange={(e) => setConfigDraft({ ...configDraft, llm_provider: e.target.value })} placeholder="llm_provider" />
                <TextField.Root value={String(configDraft.model || '')} onChange={(e) => setConfigDraft({ ...configDraft, model: e.target.value })} placeholder="model" />
                <TextField.Root value={String(configDraft.api_key || '')} onChange={(e) => setConfigDraft({ ...configDraft, api_key: e.target.value })} placeholder="api_key (leave blank to keep existing)" />
                <TextField.Root value={String(configDraft.max_tokens || 8192)} onChange={(e) => setConfigDraft({ ...configDraft, max_tokens: e.target.value })} placeholder="max_tokens" />
                <TextField.Root value={String(configDraft.max_tool_iterations || 100)} onChange={(e) => setConfigDraft({ ...configDraft, max_tool_iterations: e.target.value })} placeholder="max_tool_iterations" />
                <TextField.Root value={String(configDraft.web_host || '127.0.0.1')} onChange={(e) => setConfigDraft({ ...configDraft, web_host: e.target.value })} placeholder="web_host" />
                <TextField.Root value={String(configDraft.web_port || 3900)} onChange={(e) => setConfigDraft({ ...configDraft, web_port: e.target.value })} placeholder="web_port" />
                <TextField.Root value={String(configDraft.web_auth_token || '')} onChange={(e) => setConfigDraft({ ...configDraft, web_auth_token: e.target.value })} placeholder="web_auth_token (optional)" />
                <Flex gap="2">
                  <Button variant={Boolean(configDraft.show_thinking) ? 'solid' : 'soft'} onClick={() => setConfigDraft({ ...configDraft, show_thinking: !Boolean(configDraft.show_thinking) })}>show_thinking: {Boolean(configDraft.show_thinking) ? 'on' : 'off'}</Button>
                  <Button variant={Boolean(configDraft.web_enabled) ? 'solid' : 'soft'} onClick={() => setConfigDraft({ ...configDraft, web_enabled: !Boolean(configDraft.web_enabled) })}>web_enabled: {Boolean(configDraft.web_enabled) ? 'on' : 'off'}</Button>
                </Flex>
                {saveStatus ? (
                  <Text size="2" color={saveStatus.startsWith('Save failed') ? 'red' : 'green'}>
                    {saveStatus}
                  </Text>
                ) : null}
                <Flex justify="end" gap="2" mt="2">
                  <Dialog.Close>
                    <Button variant="soft">Close</Button>
                  </Dialog.Close>
                  <Button onClick={() => void saveConfigChanges()}>Save</Button>
                </Flex>
              </Flex>
            ) : (
              <Text>Loading...</Text>
            )}
          </Dialog.Content>
        </Dialog.Root>
      </Flex>
    </Theme>
  )
}

createRoot(document.getElementById('root')!).render(<App />)
