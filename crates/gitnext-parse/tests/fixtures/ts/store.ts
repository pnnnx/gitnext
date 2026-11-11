import { useState } from "react";
import * as fs from "node:fs";
import axios from "axios";
import { fmt } from "./util/fmt";
/** A store of tasks. */
export class Store extends Base implements IStore {
  private items: Task[] = [];
  constructor(private path: string) { super(); }
  /** Save all. */
  async save(force = false): Promise<void> { this.flush(); fs.writeFileSync(this.path, fmt(this.items)); }
  get size(): number { return this.items.length; }
}
export interface IStore { save(force?: boolean): Promise<void>; }
export type Id = string | number;
export enum Color { Red, Green }
export function load(path: string, opts?: Opts): Store { const s = new Store(path); s.save(); return s; }
export const helper = (x: number): number => x * 2;
const r = axios.get("/x").then(r => r.data);
describe("store", () => { it("saves", () => { expect(load("a")).toBeTruthy(); }); });
