<p align="center">
  <img src="../res/logo-header.svg" alt="RustDesk - Your remote desktop"><br>
  <a href="#免费的公共服务器">服务器</a> •
  <a href="#基本构建步骤">编译</a> •
  <a href="#使用-Docker-编译">Docker</a> •
  <a href="#文件结构">结构</a> •
  <a href="#截图">截图</a><br>
  [<a href="../README.md">English</a>] | [<a href="README-UA.md">Українська</a>] | [<a href="README-CS.md">česky</a>] | [<a href="README-HU.md">Magyar</a>] | [<a href="README-ES.md">Español</a>] | [<a href="README-FA.md">فارسی</a>] | [<a href="README-FR.md">Français</a>] | [<a href="README-DE.md">Deutsch</a>] | [<a href="README-PL.md">Polski</a>] | [<a href="README-ID.md">Indonesian</a>] | [<a href="README-FI.md">Suomi</a>] | [<a href="README-ML.md">മലയാളം</a>] | [<a href="README-JP.md">日本語</a>] | [<a href="README-NL.md">Nederlands</a>] | [<a href="README-IT.md">Italiano</a>] | [<a href="README-RU.md">Русский</a>] | [<a href="README-PTBR.md">Português (Brasil)</a>] | [<a href="README-EO.md">Esperanto</a>] | [<a href="README-KR.md">한국어</a>] | [<a href="README-AR.md">العربي</a>] | [<a href="README-VN.md">Tiếng Việt</a>] | [<a href="README-GR.md">Ελληνικά</a>]<br>
</p>

> [!CAUTION]
> **重要法律与合规声明：** <br>
> 严禁将 RustDesk 或 RustDeskMCP 用于任何违法、未授权、滥用、欺骗性或侵犯隐私的行为。包括但不限于：未经明确授权的远程访问或控制、绕过同意机制、窃取账号口令、监视监听、数据窃取或外传、维持权限、横向移动、勒索软件行为，以及任何规避安全审计、监管要求或执法措施的操作。上述用途一律禁止。该 fork 的使用者、集成方、部署方和分发方必须自行确保其使用具有合法依据、真实授权，并符合适用的法律法规、合同义务和内部制度要求。作者与贡献者不认可任何滥用行为，且对任何非法、未授权或侵害性部署不承担责任。

## RustDeskMCP 分支说明

本仓库是官方 [RustDesk](https://github.com/rustdesk/rustdesk) 项目的一个修改版 fork，继续遵循 AGPL-3.0 许可。它在上游 RustDesk 代码基础上增加了内置 localhost MCP 服务端、独立的 `RustDeskMCP` 应用标识，以及面向 agent 的桌面和终端控制链路。

English version of this fork overview: [../README.md](../README.md)

### 这个 fork 增加了什么

- 在桌面 Flutter 客户端内嵌 localhost MCP 服务端
- 独立的 Windows 运行标识 `RustDeskMCP`，避免和本机正常 RustDesk 安装互相抢占
- 按照正常 RustDesk 连接路径打开的 MCP 桌面会话能力
- MCP 终端会话能力
- 面向 agent 桌面任务的显式本地输入锁定与解锁语义
- 用于视觉自动化的远程桌面帧获取能力

### 这个 fork 的设计规则

- 保持原有非 MCP 的 RustDesk 用户路径可用
- 通过 GUI 开关启用 MCP，而不是依赖单独的启动模式
- MCP 操作尽量沿用 RustDesk 原本的用户可见会话路径
- 当前 MCP 范围只覆盖桌面会话和终端会话

### MCP 快速开始

1. 启动 `RustDeskMCP`
2. 在 GUI 中开启 `设置 -> 安全 -> 开启 MCP 服务器`
3. 向 `http://127.0.0.1:59940/mcp` 发送 JSON-RPC 请求
4. 先调用 `initialize`，再调用 `tools/list` 和 `tools/call`

### 当前 MCP 能力范围

桌面会话工具：

- `open_desktop_session`
- `input_password`
- `select_desktop_display`
- `get_desktop_frame`
- `lock_remote_user_input`
- `unlock_remote_user_input`
- `mouse_move`
- `mouse_click`
- `keyboard_input`
- `keyboard_hotkey`

终端会话工具：

- `open_terminal_session`
- `input_password`
- `terminal_input`
- `terminal_output`

### 上游与许可

- 上游项目：[RustDesk](https://github.com/rustdesk/rustdesk)
- 许可证：AGPL-3.0，见 [../LICENCE](../LICENCE)
- fork 说明与归属： [../NOTICE.md](../NOTICE.md)
- 当前 fork 的进一步说明： [RustDeskMCP.md](RustDeskMCP.md)

与我们交流: [知乎](https://www.zhihu.com/people/rustdesk) | [Discord](https://discord.gg/nDceKgxnkV) | [Reddit](https://www.reddit.com/r/rustdesk) | [YouTube](https://www.youtube.com/@rustdesk)

[![RustDesk Server Pro](https://img.shields.io/badge/RustDesk%20Server%20Pro-%E9%AB%98%E7%BA%A7%E5%8A%9F%E8%83%BD-blue)](https://rustdesk.com/pricing.html)

远程桌面软件，开箱即用，无需任何配置。您完全掌控数据，不用担心安全问题。您可以使用我们的注册/中继服务器，
或者[自己设置](https://rustdesk.com/server)，
亦或者[开发您的版本](https://github.com/rustdesk/rustdesk-server-demo)。

![image](https://user-images.githubusercontent.com/71636191/171661982-430285f0-2e12-4b1d-9957-4a58e375304d.png)

RustDesk 期待各位的贡献. 如何参与开发? 详情请看 [CONTRIBUTING-ZH.md](CONTRIBUTING-ZH.md).

[**FAQ**](https://github.com/rustdesk/rustdesk/wiki/FAQ)

[**BINARY DOWNLOAD**](https://github.com/rustdesk/rustdesk/releases)

[**NIGHTLY BUILD**](https://github.com/rustdesk/rustdesk/releases/tag/nightly)

[<img src="https://fdroid.gitlab.io/artwork/badge/get-it-on.png"
    alt="Get it on F-Droid"
    height="80">](https://f-droid.org/en/packages/com.carriez.flutter_hbb)

## 依赖

桌面版本使用 Flutter 或 Sciter（已弃用）作为 GUI，本教程仅适用于 Sciter，因为它更简单且更易于上手。查看我们的[CI](https://github.com/rustdesk/rustdesk/blob/master/.github/workflows/flutter-build.yml)以构建 Flutter 版本。

请自行下载Sciter动态库。

[Windows](https://raw.githubusercontent.com/c-smile/sciter-sdk/master/bin.win/x64/sciter.dll) |
[Linux](https://raw.githubusercontent.com/c-smile/sciter-sdk/master/bin.lnx/x64/libsciter-gtk.so) |
[macOS](https://raw.githubusercontent.com/c-smile/sciter-sdk/master/bin.osx/libsciter.dylib)

## 基本构建步骤

- 请准备好 Rust 开发环境和 C++ 编译环境

- 安装 [vcpkg](https://github.com/microsoft/vcpkg), 正确设置 `VCPKG_ROOT` 环境变量

  - Windows: vcpkg install libvpx:x64-windows-static libyuv:x64-windows-static opus:x64-windows-static aom:x64-windows-static
  - Linux/macOS: vcpkg install libvpx libyuv opus aom

- 运行 `cargo run`

## [构建](https://rustdesk.com/docs/en/dev/build/)

## 在 Linux 上编译

### Ubuntu 18 (Debian 10)

```sh
sudo apt install -y zip g++ gcc git curl wget nasm yasm libgtk-3-dev clang libxcb-randr0-dev libxdo-dev \
        libxfixes-dev libxcb-shape0-dev libxcb-xfixes0-dev libasound2-dev libpulse-dev cmake make \
        libclang-dev ninja-build libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev
```

### openSUSE Tumbleweed 

```sh
sudo zypper install gcc-c++ git curl wget nasm yasm gcc gtk3-devel clang libxcb-devel libXfixes-devel cmake alsa-lib-devel gstreamer-devel gstreamer-plugins-base-devel xdotool-devel
```

### Fedora 28 (CentOS 8)

```sh
sudo yum -y install gcc-c++ git curl wget nasm yasm gcc gtk3-devel clang libxcb-devel libxdo-devel libXfixes-devel pulseaudio-libs-devel cmake alsa-lib-devel
```

### Arch (Manjaro)

```sh
sudo pacman -Syu --needed unzip git cmake gcc curl wget yasm nasm zip make pkg-config clang gtk3 xdotool libxcb libxfixes alsa-lib pipewire
```

### 安装 vcpkg

```sh
git clone https://github.com/microsoft/vcpkg
cd vcpkg
git checkout 2023.04.15
cd ..
vcpkg/bootstrap-vcpkg.sh
export VCPKG_ROOT=$HOME/vcpkg
vcpkg/vcpkg install libvpx libyuv opus aom
```

### 修复 libvpx (仅仅针对 Fedora)

```sh
cd vcpkg/buildtrees/libvpx/src
cd *
./configure
sed -i 's/CFLAGS+=-I/CFLAGS+=-fPIC -I/g' Makefile
sed -i 's/CXXFLAGS+=-I/CXXFLAGS+=-fPIC -I/g' Makefile
make
cp libvpx.a $HOME/vcpkg/installed/x64-linux/lib/
cd
```

### 构建

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
git clone https://github.com/rustdesk/rustdesk
cd rustdesk
mkdir -p target/debug
wget https://raw.githubusercontent.com/c-smile/sciter-sdk/master/bin.lnx/x64/libsciter-gtk.so
mv libsciter-gtk.so target/debug
VCPKG_ROOT=$HOME/vcpkg cargo run
```

## 使用 Docker 编译

克隆版本库并构建 Docker 容器:

```sh
git clone https://github.com/rustdesk/rustdesk # 克隆Github存储库
cd rustdesk # 进入文件夹
docker build -t "rustdesk-builder" . # 构建容器
```

请注意：
* 针对国内网络访问问题，可以做以下几点优化：  
   1. Dockerfile 中修改系统的源到国内镜像
      ```
      在Dockerfile的RUN apt update之前插入两行：
   
      RUN sed -i "s|deb.debian.org|mirrors.aliyun.com|g" /etc/apt/sources.list && \
          sed -i "s|security.debian.org|mirrors.aliyun.com|g" /etc/apt/sources.list
      ```

   2. 修改容器系统中的 cargo 源，在`RUN ./rustup.sh -y`后插入下面代码：

      ```
      RUN echo '[source.crates-io]' > ~/.cargo/config \
       && echo 'registry = "https://github.com/rust-lang/crates.io-index"'  >> ~/.cargo/config \
       && echo '# 替换成你偏好的镜像源'  >> ~/.cargo/config \
       && echo "replace-with = 'sjtu'"  >> ~/.cargo/config \
       && echo '# 上海交通大学'   >> ~/.cargo/config \
       && echo '[source.sjtu]'   >> ~/.cargo/config \
       && echo 'registry = "https://mirrors.sjtug.sjtu.edu.cn/git/crates.io-index"'  >> ~/.cargo/config \
       && echo '' >> ~/.cargo/config
      ```

   3. Dockerfile 中加入代理的 env

      ```
      在User root后插入两行

      ENV http_proxy=http://host:port
      ENV https_proxy=http://host:port
      ```

   4. docker build 命令后面加上 proxy 参数

      ```
      docker build -t "rustdesk-builder" . --build-arg http_proxy=http://host:port --build-arg https_proxy=http://host:port
      ```

### 构建 RustDesk 程序

然后, 每次需要构建应用程序时, 运行以下命令:

```sh
docker run --rm -it -v $PWD:/home/user/rustdesk -v rustdesk-git-cache:/home/user/.cargo/git -v rustdesk-registry-cache:/home/user/.cargo/registry -e PUID="$(id -u)" -e PGID="$(id -g)" rustdesk-builder
```

请注意:  
* 因为需要缓存依赖项，首次构建一般很慢（国内网络会经常出现拉取失败，可以多试几次）。  
* 如果您需要添加不同的构建参数，可以在指令末尾的`<OPTIONAL-ARGS>` 位置进行修改。例如构建一个"Release"版本，在指令后面加上` --release`即可。
* 如果出现以下的提示，则是无权限问题，可以尝试把`-e PUID="$(id -u)" -e PGID="$(id -g)"`参数去掉。
   ```
   usermod: user user is currently used by process 1
   groupmod: Permission denied.
   groupmod: cannot lock /etc/group; try again later.
   ```
   > **原因：** 容器的 entrypoint 脚本会检测 UID 和 GID，在度判和给定的环境变量的不一致时，会强行修改 user 的 UID 和 GID 并重新运行。但在重启后读不到环境中的 UID 和 GID，然后再次进入判错重启环节


### 运行 RustDesk 程序

生成的可执行程序在 target 目录下，可直接通过指令运行调试 (Debug) 版本的 RustDesk:
```sh
target/debug/rustdesk
```

或者您想运行发行 (Release) 版本:

```sh
target/release/rustdesk
```

请注意：
* 请保证您运行的目录是在 RustDesk 库的根目录内，否则软件会读不到文件。
* `install`、`run`等 Cargo 的子指令在容器内不可用，宿主机才行。

## 文件结构

- **[libs/hbb_common](https://github.com/rustdesk/rustdesk/tree/master/libs/hbb_common)**: 视频编解码, 配置, tcp/udp 封装, protobuf, 文件传输相关文件系统操作函数, 以及一些其他实用函数
- **[libs/scrap](https://github.com/rustdesk/rustdesk/tree/master/libs/scrap)**: 屏幕截取
- **[libs/enigo](https://github.com/rustdesk/rustdesk/tree/master/libs/enigo)**: 平台相关的鼠标键盘输入
- **[libs/clipboard](https://github.com/rustdesk/rustdesk/tree/master/libs/clipboard)**: Windows、Linux、macOS 的文件复制和粘贴实现
- **[src/ui](https://github.com/rustdesk/rustdesk/tree/master/src/ui)**: 过时的 Sciter UI（已弃用）
- **[src/server](https://github.com/rustdesk/rustdesk/tree/master/src/server)**: 被控端服务音频、剪切板、输入、视频服务、网络连接的实现
- **[src/client.rs](https://github.com/rustdesk/rustdesk/tree/master/src/client.rs)**: 控制端
- **[src/rendezvous_mediator.rs](https://github.com/rustdesk/rustdesk/tree/master/src/rendezvous_mediator.rs)**: 与[rustdesk-server](https://github.com/rustdesk/rustdesk-server)保持UDP通讯, 等待远程连接（通过打洞直连或者中继）
- **[src/platform](https://github.com/rustdesk/rustdesk/tree/master/src/platform)**: 平台服务相关代码
- **[flutter](https://github.com/rustdesk/rustdesk/tree/master/flutter)**: 适用于桌面和移动设备的 Flutter 代码
- **[flutter/web/js](https://github.com/rustdesk/rustdesk/tree/master/flutter/web/js)**: Flutter Web版本中的Javascript代码

## 截图

![image](https://user-images.githubusercontent.com/71636191/113112362-ae4deb80-923b-11eb-957d-ff88daad4f06.png)

![image](https://user-images.githubusercontent.com/71636191/113112619-f705a480-923b-11eb-911d-97e984ef52b6.png)

![image](https://user-images.githubusercontent.com/71636191/113112857-3fbd5d80-923c-11eb-9836-768325faf906.png)

![image](https://user-images.githubusercontent.com/71636191/135385039-38fdbd72-379a-422d-b97f-33df71fb1cec.png)
