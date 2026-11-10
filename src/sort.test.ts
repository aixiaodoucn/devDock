import { describe, expect, it } from 'vitest';
import { dropIndex, moveProject } from './sort';
import { newProject, type Project } from './types';

const project = (id: string): Project => ({ ...newProject(), id, name: id, path: `/${id}` });
const list = [project('a'), project('b'), project('c'), project('d')];
const ids = (items: Project[]) => items.map((item) => item.id);

describe('项目拖拽排序', () => {
  it('移动到目标索引，向前和向后都正确', () => {
    expect(ids(moveProject(list, 'a', 2))).toEqual(['b', 'c', 'a', 'd']);
    expect(ids(moveProject(list, 'd', 0))).toEqual(['d', 'a', 'b', 'c']);
    expect(ids(moveProject(list, 'b', 3))).toEqual(['a', 'c', 'd', 'b']);
  });
  it('索引越界时收敛到有效范围，相同位置返回原数组', () => {
    expect(ids(moveProject(list, 'a', 99))).toEqual(['b', 'c', 'd', 'a']);
    expect(ids(moveProject(list, 'a', -5))).toEqual(['a', 'b', 'c', 'd']);
    expect(moveProject(list, 'a', 0)).toBe(list);
    expect(moveProject(list, 'missing', 1)).toBe(list);
  });
  it('按鼠标上下半区计算插入索引并补偿移除自身的影响', () => {
    // 把 a 拖到 c 的下半区：插入 c 之后。
    expect(dropIndex(list, 'a', 'c', true)).toBe(2);
    // 把 a 拖到 c 的上半区：插入 c 之前。
    expect(dropIndex(list, 'a', 'c', false)).toBe(1);
    // 把 d 拖到 a 的上半区：插入最前。
    expect(dropIndex(list, 'd', 'a', false)).toBe(0);
    // 把 b 拖到 a 的下半区：a 之后仍是原位置。
    expect(dropIndex(list, 'b', 'a', true)).toBe(1);
    expect(dropIndex(list, 'missing', 'a', false)).toBe(-1);
    expect(dropIndex(list, 'a', 'missing', false)).toBe(-1);
  });
  it('不修改原数组和项目对象', () => {
    const next = moveProject(list, 'a', 2);
    expect(next).not.toBe(list);
    expect(next[2]).toBe(list[0]);
    expect(ids(list)).toEqual(['a', 'b', 'c', 'd']);
  });
});
