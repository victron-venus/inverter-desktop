# Grid submeter display

The Grid heading shows the name and signed power of the backup submeter selected in inverter-control, for example `Grid · Home -750W`. Ordinary AC loads are not automatically treated as grid meters.

Enable `USE_GRID_SUBMETER_AS_BACKUP` in inverter-control to allow that selected source to take over when the primary grid meter is unavailable. The selected meter remains visible when this option is disabled. `(active)` means the controller is using the backup; unavailable readings show a dash.

The desktop consumes `inverter/state.grid_backup` and `grid_using_backup`. Clearing the selection removes the indicator. Status expires after 30 seconds without an actual daemon update, even while other Cerbo MQTT data continues to arrive. The main grid and phase readings retain their existing Cerbo source.

The Setpoint override button sends one acknowledged request to inverter-control. The daemon refreshes the requested watts every two seconds until Stop override is selected; closing desktop does not stop it.
