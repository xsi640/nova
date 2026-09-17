$designRoot = $PSScriptRoot
$sourcePath = Join-Path $PSScriptRoot 'source\preview.html'
$outputPath = Join-Path $designRoot 'output'

New-Item -ItemType Directory -Force -Path $outputPath | Out-Null
$sourceUrl = ([System.Uri]$sourcePath).AbsoluteUri

function Capture-Design([string]$screen, [string]$size, [string]$fileName) {
    $targetPath = Join-Path $outputPath $fileName
    & npx --yes playwright screenshot --channel chrome --viewport-size=$size --wait-for-timeout=250 "${sourceUrl}?screen=$screen" $targetPath
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to render $screen"
    }
}

Capture-Design 'onboarding' '1120,760' 'UI-001-onboarding.png'
Capture-Design 'chat' '440,640' 'UI-002-chat.png'
Capture-Design 'voice' '440,640' 'UI-003-voice-confirmation.png'
Capture-Design 'chat-error' '440,640' 'UI-004-chat-error.png'
Capture-Design 'memory' '1120,760' 'UI-005-memory.png'
Capture-Design 'schedule' '1120,760' 'UI-006-schedule.png'
Capture-Design 'settings' '1120,760' 'UI-007-settings.png'
Capture-Design 'notification' '800,500' 'UI-008-proactive-notification.png'
