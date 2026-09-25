param(
    [Parameter(Mandatory)][string]$Before,
    [Parameter(Mandatory)][string]$After,
    [Parameter(Mandatory)][ValidateSet('document', 'log')][string]$Region
)
$ErrorActionPreference = 'Stop'
$start = Import-Csv -LiteralPath $Before | Select-Object -Last 1
$end = Import-Csv -LiteralPath $After | Select-Object -Last 1
$fields = if ($Region -eq 'document') { @('document', 'document_rows') } else { @('log', 'log_rows') }
foreach ($field in $fields) {
    if ($null -eq $start.$field -or $null -eq $end.$field) { throw "Missing counter: $field" }
    if ([long]$start.$field -ne [long]$end.$field) {
        throw "Unexpected content rebuild in ${field}: $($start.$field) -> $($end.$field)"
    }
}
if ([long]$end.shell -le [long]$start.shell) { throw 'No UI update occurred during the check.' }
Write-Output "PASS: UI updated without rebuilding $Region content or rows."
