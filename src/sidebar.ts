import { computed, onMounted, onUnmounted, ref } from 'vue';

const STORAGE_KEY = 'dev-dock-sidebar-width';
const MIN_WIDTH = 184;
const MAX_WIDTH = 420;

export function useSidebarResize() {
  const defaultWidth = () =>
    window.innerWidth <= 900 ? 184 : window.innerWidth <= 1100 ? 196 : 208;
  const viewportWidth = ref(window.innerWidth);
  const maxWidth = computed(() =>
    Math.max(MIN_WIDTH, Math.min(MAX_WIDTH, viewportWidth.value - 600)),
  );
  const clamp = (width: number) => Math.round(Math.max(MIN_WIDTH, Math.min(maxWidth.value, width)));
  let savedWidth: number | undefined;
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    const value = raw === null ? NaN : Number(raw);
    if (Number.isFinite(value) && value >= MIN_WIDTH && value <= MAX_WIDTH) savedWidth = value;
  } catch {
    // 本地存储不可用时仍允许调整宽度。
  }
  const width = ref(clamp(savedWidth ?? defaultWidth()));
  const resizing = ref(false);
  let pointerId: number | undefined;
  let startX = 0;
  let startWidth = width.value;

  function persist() {
    try {
      localStorage.setItem(STORAGE_KEY, String(width.value));
    } catch {
      // 保存失败不影响本次窗口内的拖拽。
    }
  }
  function start(event: PointerEvent) {
    if (event.button !== 0 || resizing.value) return;
    const handle = event.currentTarget as HTMLElement;
    handle.setPointerCapture(event.pointerId);
    pointerId = event.pointerId;
    startX = event.clientX;
    startWidth = width.value;
    resizing.value = true;
  }
  function move(event: PointerEvent) {
    if (!resizing.value || event.pointerId !== pointerId) return;
    width.value = clamp(startWidth + event.clientX - startX);
  }
  function finish(event: PointerEvent) {
    if (event.pointerId !== pointerId) return;
    resizing.value = false;
    pointerId = undefined;
    persist();
  }
  function keydown(event: KeyboardEvent) {
    let next: number;
    if (event.key === 'ArrowLeft') next = width.value - 8;
    else if (event.key === 'ArrowRight') next = width.value + 8;
    else if (event.key === 'Home') next = MIN_WIDTH;
    else if (event.key === 'End') next = maxWidth.value;
    else return;
    event.preventDefault();
    width.value = clamp(next);
    persist();
  }
  function resizeWindow() {
    viewportWidth.value = window.innerWidth;
    // 窗口缩小时为主内容保留空间，但不覆盖用户保存的偏好。
    width.value = clamp(width.value);
  }
  onMounted(() => window.addEventListener('resize', resizeWindow));
  onUnmounted(() => window.removeEventListener('resize', resizeWindow));
  return { width, resizing, minWidth: MIN_WIDTH, maxWidth, start, move, finish, keydown };
}
