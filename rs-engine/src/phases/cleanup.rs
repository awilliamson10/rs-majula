use crate::engine::Engine;
use rs_entity::lifetime::EntityLifeTime;

/// Default restock interval (in ticks) for inventory slots that have
/// `allstock` enabled but no per-slot rate configured.
const INV_STOCKRATE: u64 = 100;

impl Engine {
    /// Processes the cleanup phase of the engine tick cycle.
    ///
    /// Resets per-tick transient state so that the next tick starts clean:
    ///
    /// 1. [`reset_zones`] -- clears dirty flags and per-tick buffers on
    ///    modified zones.
    /// 2. [`reset_renderers`] -- removes temporary (single-tick) renderer
    ///    entries for players and NPCs.
    /// 3. [`reset_players`] -- resets per-tick pathing-entity state on all
    ///    active players.
    /// 4. [`reset_npcs`] -- resets per-tick pathing-entity state on all
    ///    active NPCs.
    /// 5. [`remove_despawned_npcs`] -- removes NPCs with `Despawn`
    ///    lifecycle that are no longer active.
    /// 6. [`reset_shared_invs`] -- clears the per-tick change set on shared
    ///    inventories (before restock re-dirties them).
    /// 7. [`restock_invs`] -- ticks shared inventory restock timers toward
    ///    their base stock counts.
    ///
    /// # Side Effects
    ///
    /// * Clears zone tracking set.
    /// * Removes temporary renderer entries.
    /// * Resets per-tick flags on all players and NPCs.
    /// * Clears per-tick inventory change sets (player and shared).
    /// * Frees NPC slots for despawned entities.
    /// * Adjusts shared inventory item counts.
    ///
    /// # Call Stack
    ///
    /// **Called by:** `Engine::cycle`
    /// **Calls:** `reset_zones`, `reset_renderers`, `reset_players`,
    ///   `reset_npcs`, `remove_despawned_npcs`, `reset_shared_invs`, `restock_invs`
    pub(crate) fn cleanups(&mut self) {
        self.reset_zones();
        self.reset_renderers();
        self.reset_players();
        self.reset_npcs();
        self.remove_despawned_npcs();
        self.reset_shared_invs();
        self.restock_invs();
    }

    /// Resets all zones that were modified during this tick.
    ///
    /// Drains `self.zones_tracking` and calls `reset()` on each zone,
    /// clearing per-tick update buffers and dirty flags.
    ///
    /// # Side Effects
    ///
    /// * Clears `self.zones_tracking`.
    /// * Resets per-tick state on modified zones.
    fn reset_zones(&mut self) {
        for coord in self.zones_tracking.drain() {
            if let Some(zone) = self.zones.zones.get_mut(&coord) {
                zone.reset();
            }
        }
    }

    /// Removes temporary (single-tick) entries from the player and NPC
    /// renderers.
    ///
    /// Temporary entries are used for entities that are only visible for a
    /// single tick (e.g. during initial add). After the output phase has
    /// transmitted them, they must be cleaned up.
    ///
    /// # Side Effects
    ///
    /// * Modifies `self.player_renderer` and `self.npc_renderer`.
    fn reset_renderers(&mut self) {
        let pids = self.player_list.take_pids();
        self.player_renderer.remove_temporary(&pids);
        self.player_list.put_pids(pids);
        let nids = self.npc_list.take_nids();
        self.npc_renderer.remove_temporary(&nids);
        self.npc_list.put_nids(nids);
    }

    /// Resets per-tick pathing-entity state on all active players.
    ///
    /// Calls `reset_pathing_entity(false)` which clears transient flags
    /// such as step counters and movement deltas without resetting
    /// persistent state like coordinates. Also clears each player's
    /// per-tick inventory change set now that the output phase has
    /// transmitted any dirty slots (full or partial) to all viewers.
    ///
    /// # Side Effects
    ///
    /// * Clears per-tick movement and info flags on each player.
    /// * Clears the dirty flag and changed-slot set on each player inventory.
    fn reset_players(&mut self) {
        for &pid in self.player_list.processing.iter() {
            let Some(active) = self.player_list.players[pid as usize].as_mut() else {
                continue;
            };
            active.player.reset_pathing_entity(false);
            for inv in active.player.invs.values_mut() {
                if inv.dirty {
                    inv.clear_dirty();
                }
            }
        }
    }

    /// Resets per-tick pathing-entity state on all active NPCs.
    ///
    /// Calls `reset_pathing_entity(false)` which clears transient flags
    /// such as step counters and movement deltas without resetting
    /// persistent state like coordinates.
    ///
    /// # Side Effects
    ///
    /// * Clears per-tick movement and info flags on each NPC.
    fn reset_npcs(&mut self) {
        for &nid in self.npc_list.processing.iter() {
            let Some(active) = self.npc_list.npcs[nid as usize].as_mut() else {
                continue;
            };
            active.npc.reset_pathing_entity(false);
        }
    }

    /// Removes fully despawned NPCs from the active list and frees their
    /// slots.
    ///
    /// An NPC is considered despawned when it is inactive and has
    /// [`EntityLifeTime::Despawn`] lifecycle (i.e. it was dynamically
    /// spawned and has completed its death sequence). The slot in
    /// `self.npcs` is set to `None` and the NPC is removed from
    /// `self.active_npcs`.
    ///
    /// # Side Effects
    ///
    /// * Frees NPC slots in `self.npcs`.
    /// * Shrinks `self.active_npcs`.
    fn remove_despawned_npcs(&mut self) {
        let nids = self.npc_list.take_nids();
        for &nid in &nids {
            let should_remove = self.npc_list.npcs[nid as usize]
                .as_ref()
                .is_some_and(|n| !n.npc.active && n.npc.lifecycle == EntityLifeTime::Despawn);
            if should_remove {
                self.npc_list.remove(nid);
            }
        }
        self.npc_list.put_nids(nids);
    }

    /// Clears the per-tick change set on every shared (world) inventory.
    ///
    /// Runs after the output phase has transmitted any dirty slots to all viewers,
    /// but *before* [`restock_invs`](Self::restock_invs) -- restocking re-dirties
    /// shared inventories, and those changes must survive into the next tick's
    /// output so they are transmitted.
    ///
    /// # Side Effects
    ///
    /// * Clears the dirty flag and changed-slot set on each shared inventory.
    fn reset_shared_invs(&mut self) {
        for inv in self.invs.values_mut() {
            if inv.dirty {
                inv.clear_dirty();
            }
        }
    }

    /// Ticks shared (world) inventory restock timers.
    ///
    /// For each restockable inventory, checks each slot against its base
    /// stock count. When the tick aligns with the slot's configured
    /// restock rate:
    ///
    /// * If the current count is below base, increments by 1.
    /// * If the current count is above base, decrements by 1.
    /// * If `allstock` is enabled and the base count is 0, decrements
    ///   excess items at the default [`INV_STOCKRATE`].
    ///
    /// Empty slots are skipped. Sold-out stock items never reach this state: the
    /// removal path keeps a stock item in its slot at count 0 (see `Inventory`),
    /// so the increment branch above restocks it back over time.
    ///
    /// # Side Effects
    ///
    /// * Modifies item counts in shared inventories.
    fn restock_invs(&mut self) {
        let tick = self.clock;
        let cache = self.cache;
        for (&inv_id, inv) in &mut self.invs {
            let inv_type = match cache.invs.get_by_id(inv_id) {
                Some(t) => t,
                None => continue,
            };
            if !inv_type.restock {
                continue;
            }
            let (Some(stockcount), Some(stockrate)) = (&inv_type.stockcount, &inv_type.stockrate)
            else {
                continue;
            };
            let allstock = inv_type.allstock;

            for index in 0..inv.capacity {
                let Some(item) = inv.get(index as u16).copied() else {
                    continue;
                };
                let base_count = stockcount.get(index).copied().unwrap_or(0) as u32;
                let rate = stockrate.get(index).copied().unwrap_or(0);

                // Item stock is under min -> restock one unit toward base.
                if item.num < base_count && rate > 0 && tick.is_multiple_of(rate as u64) {
                    inv.set(index as u16, item.obj, item.num + 1);
                    continue;
                }

                // Item stock is over min -> destock one unit toward base.
                if item.num > base_count && rate > 0 && tick.is_multiple_of(rate as u64) {
                    inv.remove(index as u16, 1);
                    continue;
                }

                if allstock && base_count == 0 && tick.is_multiple_of(INV_STOCKRATE) {
                    inv.remove(index as u16, 1);
                }
            }
        }
    }
}
