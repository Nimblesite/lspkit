param([string]$Source)
# Holds an exclusive byte-range lock over $Source, so another process can open
# the file but its first read fails with ERROR_LOCK_VIOLATION. Windows has no
# EISDIR; this is its "opens, then cannot be read" source. [LSP-PULL-TRANSPORT]
$ErrorActionPreference='Stop'
$stream=[IO.File]::Open($Source,'Open','ReadWrite','ReadWrite')
try {
 $stream.Lock(0,[Math]::Max(1,$stream.Length))
 Write-Output '{"state":"locked"}'
 $null=[Console]::In.ReadLine()
} finally { $stream.Dispose() }
