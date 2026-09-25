param(
    [Parameter(Mandatory)][string]$Before,
    [Parameter(Mandatory)][string]$After
)
$ErrorActionPreference = 'Stop'
$start = Import-Csv -LiteralPath $Before | Select-Object -Last 1
$end = Import-Csv -LiteralPath $After | Select-Object -Last 1
if (!$start -or !$end) { throw 'Both files must contain a UI counter snapshot.' }
foreach ($field in @('header', 'sidebar', 'transcript', 'chat_rows', 'message_rows')) {
    if ([long]$end.$field -ne [long]$start.$field) {
        throw "Typing invalidated ${field}: $($start.$field) -> $($end.$field)"
    }
}
if ([long]$end.composer -le [long]$start.composer) {
    throw 'The composer did not render; the test did not exercise an input update.'
}
Write-Output 'PASS: composer updated; cached siblings and virtual rows were reused.'
