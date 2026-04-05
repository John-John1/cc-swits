# Windows 客户端构建经验

这份文档记录 `E:\cc_switch\source` 在 Windows 上构建客户端时的直线路径。当前推荐目标已经改成：

- 只构建 `cc-switch.exe`
- 所有构建产物都放到 `E:\cc_myself`
- 不再把 `dist`、`target`、安装包等非源码内容写回仓库目录

## 推荐命令

在仓库根目录执行：

```powershell
cd E:\cc_switch\source
cmd /c pnpm install
cmd /c pnpm typecheck
cmd /c pnpm run build:win:exe
```

## 这条命令会做什么

`pnpm run build:win:exe` 会调用：

- [build-windows-exe.ps1](E:/cc_switch/source/scripts/build-windows-exe.ps1)

脚本会自动完成下面这些事：

- 调用 `D:\huanjing\Common7\Tools\LaunchDevCmd.bat` 进入 MSVC 构建环境
- 给当前构建会话补上 `%USERPROFILE%\.cargo\bin`
- 把 Vite 产物输出到 `E:\cc_myself\cc-switch\dist`
- 把 Rust / Tauri 产物输出到 `E:\cc_myself\cc-switch\target`
- 使用 `pnpm tauri build --no-bundle`，只生成 exe，不生成 `msi` / `nsis`
- 关闭 `createUpdaterArtifacts`，避免本机构建因为缺少 `TAURI_SIGNING_PRIVATE_KEY` 失败
- 最终把 exe 复制到 `E:\cc_myself\cc-switch.exe`

## 产物位置

本地构建成功后，重点看这些路径：

- `E:\cc_myself\cc-switch.exe`
- `E:\cc_myself\cc-switch\dist`
- `E:\cc_myself\cc-switch\target\release\cc-switch.exe`

其中：

- `E:\cc_myself\cc-switch.exe` 是最方便直接拿去运行的成品
- `E:\cc_myself\cc-switch\...` 是本次构建产生的全部中间产物和缓存

## 前置条件

### Node / pnpm

- `node` 可用
- `pnpm` 可用
- 已执行过 `pnpm install`

### Rust

- 机器上已经安装 Rust
- `cargo` 实际位于 `%USERPROFILE%\.cargo\bin`
- 仓库要求 `Rust 1.85.0+`

### Windows C++ 构建环境

- 机器上存在 Visual Studio Build Tools
- 当前验证可用入口：
  - `D:\huanjing\Common7\Tools\LaunchDevCmd.bat`

## 已踩过的坑

### 1. `cargo metadata` program not found

表现：

```text
failed to run 'cargo metadata' command to get workspace directory: program not found
```

原因：

- Rust 没装
- 或者 `cargo` 不在当前 shell 的 PATH

直达处理：

- 安装 Rust
- 或者在构建命令里显式补 `%USERPROFILE%\.cargo\bin`
- 同时用 `LaunchDevCmd.bat` 带起 MSVC 环境

### 2. `spawn EPERM`

表现：

```text
Error: spawn EPERM
```

这次经验里，通常不是源码问题，而是执行环境权限或沙箱问题。优先确认：

- 当前 shell 是否受限
- `node_modules` 是否完整
- `esbuild.exe` 是否能单独运行

### 3. updater 私钥缺失导致打包失败

表现：

```text
A public key has been found, but no private key.
```

原因：

- `tauri.conf.json` 里启用了 updater 产物
- 本机没有配置 `TAURI_SIGNING_PRIVATE_KEY`

当前推荐解法：

- 对“本地验证 exe 能否构建成功”的目标，不走 installer / updater
- 直接使用 `build:win:exe`

## 下次直接照着走

如果目标只是本地生成一个可运行的 Windows 客户端，直接执行：

```powershell
cd E:\cc_switch\source
cmd /c pnpm install
cmd /c pnpm typecheck
cmd /c pnpm run build:win:exe
```

然后只检查：

```text
E:\cc_myself\cc-switch.exe
```
