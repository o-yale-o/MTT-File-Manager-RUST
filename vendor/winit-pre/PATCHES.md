# winit 0.30.13 vendor patch — 混合 DPI 双显示器拖动修复

## 症状

按住标题栏把窗口从一台显示器拖到另一台**缩放比例不同**的显示器时：

- 跨过屏幕边界瞬间窗口尺寸变一次（触发 DPI 重算）；
- 但位置被"弹回"或卡在屏幕边界，跨屏移动永远完不成；
- 缩放一致的显示器之间拖动正常。

## 根因

`WM_DPICHANGED` 的 `lparam` 是 Windows 按目标显示器坐标系预先算好的**建议矩形**
（位置 + 尺寸，保证光标相对标题栏位置不变）。winit 0.30.13 的处理：

1. 丢弃建议矩形的位置，用"旧物理尺寸 ÷ 旧缩放 × 新缩放"自行重算尺寸；
2. 后续的显示器间 nudge 块中 `MonitorFromWindow` 此刻仍返回**源显示器**，
   比较永远不匹配，于是把矩形往源显示器方向推——窗口卡在跨边界位置。

上游 issue：
- https://github.com/rust-windowing/winit/issues/4573
- https://github.com/rust-windowing/winit/issues/4041
- https://github.com/rust-windowing/winit/issues/4600

## 补丁内容

`src/platform_impl/windows/event_loop.rs` 的 `WM_DPICHANGED` 分支：当**拖动中**
（`MARKER_IN_SIZE_MOVE` 置位）且 `allow_resize`（非最大化/全屏）时，跳过
"自行重算尺寸 + 显示器 nudge"整段逻辑，`new_outer_rect` 直接采用 `lparam`
系统建议矩形；上游其余簿记（`ScaleFactorChanged` 事件、MAXIMIZED 标志处理、
唯一的 SetWindowPos 调用）全部保留。

> 实现备注：最初版本采用"提前 SetWindowPos 建议矩形并 return"，实测在
> 25%→50% 方向拖动时窗口进入 outline-drag（虚框）且文字尺寸不同步——
> 提前 return 跳过了上游簿记导致内部状态与实际窗口不一致。当前版本把
> 建议矩形注入上游自己的 SetWindowPos 流程，簿记完整保留，两个方向均正常。
> 参照 Argus（GPUI Win32 后端）的原则：不自己算尺寸、不做 nudge。

基线：crates.io `winit 0.30.13` 源码原样拷贝，仅上述一处条件包裹；上游 LICENSE 保留。

## 验证

无头环境测不了，需手工：

- [ ] 两个方向的跨屏拖动都能完成（A→B、B→A），拖回原显示器正常；
- [ ] 跨屏过程中窗口尺寸合理，UI 按目标显示器缩放清晰渲染；
- [ ] 静止状态改显示缩放比例，行为与打补丁前一致；
- [ ] 最大化窗口跨屏（Win+Shift+方向键）不劣化；
- [ ] 全量测试套件无新增失败。

## 移除条件

eframe 升级到带修复的 winit（0.31+）后：删除 `vendor/winit-pre/` 与
`Cargo.toml` 中 `[patch.crates-io]` 的 winit 条目，重跑上述验证。
