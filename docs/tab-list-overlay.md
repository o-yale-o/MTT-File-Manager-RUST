# 页签管理列表（360 风格弹出列表框）实现全记录

> 目标读者：未来想在本项目（或任何 egui 项目）里再做一个"点按钮弹出、填满窗口剩余高度、
> 行内带 ✕ 按钮、点外部一次点击直达"这类列表浮层的人。
> 本文整理了完整需求规格、最终代码位置、实现步骤、以及一路踩过的所有坑。

---

## 1. 最终行为规格（验收标准）

| 交互 | 行为 |
|---|---|
| 触发按钮 | 页签 ≥ 2 个时才显示；36×36、与「+」按钮同规格、顶部对齐；框内显示页签数 |
| 弹层几何 | 宽度 320（定死）；右边缘紧贴主窗口右缘；顶部 = 按钮(页签行)底部 + 2px；高度 = 页签栏底部 → 状态栏顶部（**固定，不随内容缩水**，超长列表内部滚动） |
| 行结构 | 灰色"其它"固定首行（行点击无响应，其"×"= 关闭除激活页外全部页签）+ 每页签一行 |
| 行交互 | 点行 = 切换该页签（活动页签行点了无动作）；行尾"×"= 只关该页签 |
| 开关规则 | 列表内点击**不**关闭；点列表**外**一次点击 = 底下控件照常生效 + 列表关闭（不吞点击，360 式） |
| 页签内容同步 | 每帧按当前页签集合重渲染（新增/关闭/切分组即时反映）；虚拟路径用本地化名称（"此电脑"而非 `::computer::`） |
| 附加 | 「标签管理」按钮 tooltip；min/max/close 也有 Win11 同款 tooltip（最大化后显示"还原"） |

---

## 2. 代码位置

| 文件 | 职责 |
|---|---|
| `src/ui/tab_bar/window_controls.rs` | 全部核心：按钮、浮层 Area、行渲染、外部点击检测、tooltip |
| `src/ui/tab_bar/window_controls_tests.rs` | egui 交互单元测试（模拟真实点击节奏） |
| `src/ui/tab_bar/mod.rs` | 调用点：组装 `tab_titles`（走 `display_title_for_tab_path`）、传参 |
| `src/ui/tab_bar/tabs_renderer.rs` | 页签本体：双击关闭 |
| `src/ui/tab_bar/strip.rs` | 布局：为按钮/列表预留 `WINDOW_BUTTON_WIDTH * 4` 与 `LOGO_AREA_WIDTH` |
| `src/ui/app/layers/status_bar_layer.rs` | 每帧把状态栏矩形写进 `ctx.data`（键 `status_bar_rect`），供浮层算高度下界 |
| `src/ui/app/layers/tab_bar_layer.rs` | `TabBarAction::CloseOtherTabs(keep)` 的实际关页签逻辑 |
| `src/app/operations/context_menu.rs` | `app_icon_texture`（列表旁路：左上角 logo 复用同一缓存） |
| `locales/*.yml` | `tab_bar.others / manage_tabs / minimize / maximize / restore / close` |

---

## 3. 浮层的最终形态（可直接抄的骨架）

```rust
// ① 按钮本身：手动定位（不参与 cursor 布局），显式 Id
let list_id = ui.make_persistent_id("window_tab_list_button");
let list_rect = egui::Rect::from_min_size(
    egui::pos2(scope_rect.left(), scope_rect.top() + top_padding),
    vec2(NEW_TAB_WIDTH, btn_height),
);
let list_response = ui.interact(list_rect, list_id.with("hit"), egui::Sense::click())
    .on_hover_text(rust_i18n::t!("tab_bar.manage_tabs"));
// ↑ 关键：响应 Id 用 list_id.with("hit")，弹层 Id 用 list_id —— 两者绝不能相同！
//   （同 Id 会让按钮的点击状态和弹层开关状态互相踩，表现为"点了弹不出来"）

// ② 开关状态持久化在 ctx.data
let mut open = ctx.data(|d| d.get_temp::<bool>(list_id).unwrap_or(false));
if list_response.clicked() { open = !open; ctx.data_mut(|d| d.insert_temp(list_id, open)); }

// ③ 弹层本体：自绘 Area（不要用 egui::Popup::menu，见坑 #1）
let panel_rect = egui::Rect::from_min_size(origin, vec2(LIST_WIDTH, list_height));
egui::Area::new(list_id)
    .fixed_pos(panel_rect.min)
    .order(egui::Order::Foreground)
    .show(&ctx, |ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(LIST_WIDTH, list_height), egui::Sense::hover());
        // 画底、画边框，然后 new_child 一个内部 Ui 放 ScrollArea + 行
    });

// ④ 外部按下检测（不吞点击！）：没有 backdrop，直接查指针
let (press_started, press_pos) = ctx.input(|i| (i.pointer.primary_pressed(), i.pointer.interact_pos()));
if press_started
    && press_pos.map(|p| !panel_rect.contains(p) && !list_rect.contains(p)).unwrap_or(false)
{
    open = false; // 这次按下照常落到下层控件（页签照切），列表同时关闭
}

// ⑤ 每帧把面板层重新置顶（防死锁，见坑 #3）
ctx.memory_mut(|mem| mem.areas_mut()
    .move_to_top(egui::LayerId::new(egui::Order::Foreground, list_id)));
```

行（360 风格一体化）：

```rust
// 行矩形先算出来（不注册任何 widget！），用它做 hover 判断和背景绘制
let row_rect = egui::Rect::from_min_size(
    pos2(ui.max_rect().left(), ui.cursor().top()),
    vec2(ui.available_width(), ROW_HEIGHT));           // ROW_HEIGHT = 26
let row_hovered = ui.rect_contains_pointer(row_rect);
let painter = ui.painter_at(row_rect);                 // 行级裁剪
if is_active { painter.rect_filled(row_rect, 4.into(), selection); }
else if row_hovered { painter.rect_filled(row_rect, 4.into(), hover_bg); }

ui.horizontal(|ui| {
    ui.spacing_mut().item_spacing.x = 0.0;             // 行内零间距
    // 主文本留空，文字全部走 left_text（左对齐 + 不出重复）
    let label = ui.add_sized([row_rect.width() - X_ZONE, ROW_HEIGHT],
        egui::Button::selectable(is_active, "").frame(false).left_text(text));
    if label.clicked() { picked = Some(index); }
    if row_hovered {                                    // ✕ 仅 hover/选中时存在
        let x_btn = ui.add(egui::Button::new(RichText::new("✕").size(11.0).color(x_color))
            .small().frame(false).min_size(vec2(X_ZONE, ROW_HEIGHT)));
        if x_btn.clicked() { picked_close = Some(index); }
    }
});
// 注意：最后必须让 cursor 越过 row_rect —— 用 allocate_space/子 Ui 的布局自然推进即可，
// 千万不要再额外 interact 同矩形（见坑 #5）。
```

---

## 4. 坑点清单（按踩到的顺序，每条都是真踩过）

### #1 `egui::Popup::menu` 不认 `.anchor()` / `.width()`
想"宽 320、右贴窗缘、固定高度"，`Popup::menu(&resp).width(320).anchor(pos)` 全被无视，
弹出位置/尺寸完全不对。**结论：这类几何自由度直接自绘 `egui::Area`**，
放弃 Popup（Popup 适合"贴着按钮的普通菜单"）。

### #2 Area 会被"点击置顶"→ backdrop 死锁（最大的坑）
egui 规则：**被点击的 Area 自动 `move_to_top`**（为可拖动浮窗设计）。
全屏 backdrop 也是 Area——用户在列表开着时点了任何列表外的东西（哪怕是侧栏切分组），
backdrop 被置顶到面板之上，之后**所有**行点击都被 backdrop 吃掉：
表现 = "点行没反应 + 点一下列表就关"，且永不自愈（每次点 backdrop 它又置顶一次）。
**修复**：列表打开的每一帧，在两个 Area 都 show 完之后：
```rust
ctx.memory_mut(|m| m.areas_mut().move_to_top(egui::LayerId::new(Order::Foreground, list_id)));
```
只在一帧里置顶没用（下一次点击又翻车），必须**每帧**。也试过"只在点按钮打开时置顶"——
不行，因为死锁状态下点按钮那一下本身就先被 backdrop 吃掉了，置顶代码根本没机会跑。

### #3 吞点击的 backdrop = "要点两次"
backdrop 用 `Sense::click` 拦截外部点击来实现"点外关闭"，副作用是那一次点击不会到达
下层控件——用户要点两次才能点到页签。**修复**：彻底去掉 backdrop，
改用 `pointer.primary_pressed()` + 指针位置判断"按下面板外"→ 关列表，
按下本身照常派发给下层。一次点击 = 生效 + 关闭。

### #4 egui 命中测试对"重叠矩形"取最上层
把"×"的 interact 矩形叠在行矩形**内部**（同一层），行交互会失效/错乱——
表现为行点击时灵时不灵、"×"永远收不到。**结论：同一层内绝不让可点击矩形互相重叠**，
"×"和行标签并排放（各自独占矩形）。

### #5 同一矩形注册两次 interact = 所有行叠在一起
用 `ui.interact(row_rect, ...)` 画行却忘了推进布局光标——每行的矩形都是第一行的位置，
点击永远命中最上层（最后创建）的行。**结论**：先 `allocate_space`（推进光标、不注册
widget）或让子控件自然布局，再对"确定的矩形"做 interact。

### #6 egui 点击命中用的是**上一帧**的控件矩形
按下帧里"移动+按下"同时发生时，若该指针位置上一帧没有对应控件，点击不注册。
真实用户移动鼠标必然有悬停帧所以永远正常；**但测试模拟点击时必须在按下帧之前
插入一帧 PointerMoved（hover 帧）**，否则测试全是假失败。

### #7 `UiBuilder::sense(...)` 会吞子控件点击
给 new_child 的 Ui 设 `Sense::hover/click` 会让这个 Ui 自己参与命中，可能挡住里面
的按钮。**结论：内部容器不要设 sense。**

### #8 `Button::selectable(text)` + `.left_text(text)` = 文字出现两份
`selectable` 的主文本本身就是一份，`left_text` 再加一份。
**结论：主文本传 `""`，文字全部放 `left_text`（顺带天然左对齐）。**

### #9 Id 冲突：按钮响应与弹层用同一个 Id
`ui.interact(rect, list_id, ...)` + `Popup/Area .id(list_id)` ——点击状态和弹层开关
状态互相覆盖，"点了弹不出来"。**结论：响应 Id 用派生 Id（`list_id.with("hit")`）。**

### #10 浮层里的文字要显示"此电脑"而不是 `::computer::`
页签的原始 title 对虚拟视图是内部 id。**必须复用页签条同一套映射**：
`appearance::display_title_for_tab_path(&tab.path, &tab.title, tag_definitions)`，
列表和页签条的显示永远一致。

### #11 `UiBuilder::sense` 之外的小坑杂项
- `Area` 里 `ui.set_min_size(...)` 用了 `Rect::NOTHING.size()` 会得到 `-inf` → panic
  （测试里 `RawInput::default()` 的 screen_rect 是 None 时的连锁反应）。
- `allocate_exact_size` 不会推进横向布局光标到自定义位置——手动定位用
  `ui.interact(rect, ...)` 后要自己补 `allocate_exact_size` 占位推进光标。
- Windows GUI 子系统下 `eprintln!` 在 PowerShell 里**看不见**——别指望 println 调试
  这种 exe，要么用文件日志要么写单元测试。

---

## 5. 测试写法（`window_controls_tests.rs` 的模式）

```rust
// 铁律：每次按下之前必须有一帧只发 PointerMoved（hover 帧），见坑 #6。
fn click(ctx, titles, active, pos, action) {
    render(ctx, titles, active, hover(pos), action);   // hover 帧
    render(ctx, titles, active, input(pos, true), action);   // 按下帧
    render(ctx, titles, active, input(pos, false), action);  // 释放帧
    render(ctx, titles, active, plain(), action);            // 空闲帧
}
```
- `RawInput` 必须带 `screen_rect`（否则一切矩形是 NOTHING，连锁 -inf panic）。
- 几何常量集中在测试顶部（`ROW_Y` 等），控件几何一改先改这里。
- 断言只看 `action`；注意"点中当前活动页签 = 设计上的 no-op"不是 bug。

---

## 6. 设计取舍备忘

- **弹层高度固定不缩水**：360 的观感核心是"面板永远占满标题栏~状态栏"。
  内容不足就留白（`ui.set_min_height(list_height)`），不要 `auto_shrink`。
- **列表内点击不关列表**：连续操作（切多个页签/关多个）不用重新打开；
  关闭入口只有"点外部"和"再点一次按钮"。
- **虚拟路径显示名**：永远走 `display_title_for_tab_path`，别自己拼。
- **行高暂不随字体自适应**（26px 固定 + 13pt 字号，比例恒定不会破）。
  以后要自适应：行高改 `fonts.row_height(13pt) + 12`，✕ 区域跟随即可，改动很局部。
