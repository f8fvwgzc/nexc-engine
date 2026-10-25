---
id: emb-buses-and-communication
name: Field buses and communication
category: embedded
description: Selects and configures embedded and industrial links: CAN and CAN FD bit timing and bus load, CANopen, EtherCAT, SPI, I2C, UART, Ethernet and TSN, with timing and wiring checks.
keywords: can bus, can fd, bit timing, canopen, ethercat, spi, i2c, uart, rs-485, ethernet, tsn, fieldbus, bus load, cia 301, iso 11898, 802.1qbv
source: CAN in Automation - CAN FD, the basic idea | https://www.can-cia.org/can-knowledge/can-fd-the-basic-idea
source: CAN in Automation - CANopen | https://www.can-cia.org/can-knowledge/canopen
source: EtherCAT Technology Group - Technology | https://www.ethercat.org/en/technology.html
source: IEEE 802.1 Time-Sensitive Networking Task Group | https://1.ieee802.org/tsn/
---
## Choosing a bus
| Bus | Use | Rate | Topology | Watch |
|---|---|---|---|---|
| UART / RS-485 | console, Modbus RTU | to ~1 Mbit/s | point-to-point / multidrop | total baud error under ~2% |
| I2C | on-board sensors | 100k/400k/1M | shared, <= 400 pF (550 pF Fm+) | pull-ups, address clashes, clock stretching |
| SPI | ADC, flash, display | tens of MHz | chip select per device | CPOL/CPHA mode, no ack, add CRC |
| CAN CC | vehicles, machines | <= 1 Mbit/s | line, 120 ohm at both ends | 8-byte payload |
| CAN FD | ECUs, CANopen FD | data 2-5 Mbit/s | line | 64-byte payload |
| EtherCAT | servo motion | 100 Mbit/s | line, tree, star | cycles <= 100 us |
| Ethernet + TSN | converged OT/IT | 10M-10G | switched | time sync, schedules |
Rule: pick the slowest, simplest link that meets latency, distance, node count and diagnostics.

## CAN and CAN FD
- ISO 11898-1:2024 (CAN CC, CAN FD, CAN XL data link); ISO 11898-2:2024 (high-speed transceivers).
- tq = BRP/f_clk; bit = SYNC (1 tq) + PROP + PHASE1 + PHASE2; sample point = (1 + PROP + PHASE1)/total tq. Arbitration SP about 87.5% (CiA practice); SJW <= min(PHASE1, PHASE2), as large as allowed. Data phase: lower SP (~70-80%) plus transmitter delay compensation at high rates. Same tq in both phases where possible; CiA 601-3 gives the rules.
- PROP must cover the round trip: 2 x (cable ~5 ns/m + transceiver + controller delays); this caps length versus rate.
- Arbitration is bitwise and non-destructive; lowest ID wins. Assign IDs by deadline.
- Worst-case classic frame, 11-bit ID, n data bytes: 47 + 8n + floor((34 + 8n - 1)/4) bits; 135 bits for n = 8.
- Bus load = sum(bits per frame x frames/s)/bit rate; design to <= 50-70% and check worst-case latency by response-time analysis: R = J + w + C, w = B + sum over higher-priority k of ceil((w + J_k + tau_bit)/T_k) x C_k, B = longest lower-priority frame, iterate to a fixed point.
- CAN FD flags FDF, BRS (rate switches at the BRS sample point), ESI; DLC 9-15 = 12, 16, 20, 24, 32, 48, 64 bytes; CRC-17 up to 16 bytes, CRC-21 above, plus stuff-bit counter.
- Fault confinement: error passive at TEC or REC >= 128, bus-off when TEC > 255.

## CANopen
- CiA 301 (v4.2.0) application layer; CiA 1301 for CANopen FD; CiA 402 drives.
- Object dictionary: 1000h-1FFFh communication, 2000h-5FFFh manufacturer, 6000h-9FFFh profiles.
- Predefined COB-IDs: NMT 000h, SYNC 080h, EMCY 080h+node, TIME 100h, TPDO1-4 180h/280h/380h/480h+node, RPDO1-4 200h/300h/400h/500h+node, SDO 580h+node (server to client) and 600h+node, heartbeat 700h+node.
- NMT: initialisation -> pre-operational -> operational or stopped; PDOs run only when operational; heartbeat producer 1017h, consumer 1016h.

## EtherCAT
Frames processed on the fly (EtherType 0x88A4); distributed clocks with jitter well under 1 us; states INIT -> PRE-OP -> SAFE-OP -> OP; mailbox CoE, FoE (firmware), EoE, SoE; FSoE black channel to IEC 61784-3, up to SIL 3; standardised in IEC 61158.

## Ethernet and TSN
IEEE 802.1AS-2025 (gPTP time), 802.1Q-2022 (Qbv time-aware shaper, Qbu preemption with 802.3br, Qci policing), 802.1CB (redundant frames), profiles IEC/IEEE 60802 (industrial) and 802.1DG-2025 (automotive). 10BASE-T1S/T1L single-pair Ethernet reaches field devices.

## Checklist
Termination and stub length; common ground or isolation; I2C pull-up (VDD - 0.4 V)/I_OL <= R <= t_r/(0.8473 x C_bus), t_r max 1000/300/120 ns, I_OL 3 mA (20 mA Fm+); SPI clock versus trace length; checksum on every frame; error counters exposed in diagnostics.

## Output
Bus choice with reasons; bit-timing table per phase (clock, BRP, segments, sample point, SJW); ID or PDO map with periods and deadlines; bus-load and worst-case latency results; wiring, termination and diagnostics notes.
