$bin = "$env:USERPROFILE\.rustup\toolchains\stable-x86_64-pc-windows-gnu\lib\rustlib\x86_64-pc-windows-gnu\bin"
$env:PATH = "$env:USERPROFILE\.cargo\bin;$bin\self-contained;$bin\gcc-ld;$env:PATH"
