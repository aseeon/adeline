param([Parameter(Mandatory)][int]$ProcessId)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

# Run against an open Chats view, with a project selected and no dialog open.
$process = New-Object System.Windows.Automation.PropertyCondition(
    [System.Windows.Automation.AutomationElement]::ProcessIdProperty, $ProcessId)
$windows = [System.Windows.Automation.AutomationElement]::RootElement.FindAll(
    [System.Windows.Automation.TreeScope]::Children, $process)
foreach ($window in $windows) {
    $message = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::NameProperty, 'Message')
    $messageField = $window.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $message)
    if (!$messageField) { continue }
    $viewport = $window.Current.BoundingRectangle
    $panels = @('Conversation list panel', 'Agent activity panel')
    $composerControls = @('Message', 'Send', ('Attach files' + [char]0x2026), 'Agent selection')
    $conversation = $null
    $sidebar = $null
    foreach ($name in (@('Conversation panel') + $composerControls + @('Chat settings', 'Agent activity') + $panels)) {
        $named = New-Object System.Windows.Automation.PropertyCondition(
            [System.Windows.Automation.AutomationElement]::NameProperty, $name)
        $element = $window.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $named)
        if (!$element) {
            if ($name -in $panels) { continue }
            throw "Missing retained control: $name"
        }
        $bounds = $element.Current.BoundingRectangle
        if ($bounds.IsEmpty -or $bounds.Width -le 0 -or $bounds.Height -le 0 -or
            !$viewport.Contains($bounds)) {
            throw "$name is outside the window: $bounds (window $viewport)"
        }
        if ($name -eq 'Conversation panel') { $conversation = $bounds }
        if ($name -eq 'Conversation list panel') { $sidebar = $bounds }
        if ($name -in $composerControls -and !$conversation.Contains($bounds)) {
            throw "$name is outside the conversation panel: $bounds (panel $conversation)"
        }
    }
    if ($sidebar) {
        $buttonType = New-Object System.Windows.Automation.PropertyCondition(
            [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
            [System.Windows.Automation.ControlType]::Button)
        foreach ($button in $window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $buttonType)) {
            if ($button.Current.Name -like 'Show * chats, * total' -and
                !$sidebar.Contains($button.Current.BoundingRectangle)) {
                throw "Chat filter is outside its sidebar: $($button.Current.Name)"
            }
        }
    }
    Write-Output 'PASS: composer controls fit their panel; visible panels and bottom controls fit the native window.'
    exit 0
}
throw 'Open a project in Chats before running the layout check.'
