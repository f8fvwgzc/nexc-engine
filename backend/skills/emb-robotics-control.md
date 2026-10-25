---
id: emb-robotics-control
name: Robotics control and kinematics
category: embedded
description: Solves robot kinematics and control problems: DH models, forward and inverse kinematics, Jacobians and singularities, PID and feedforward, trajectory and motion planning in ROS 2, and ISO 10218 and ISO/TS 15066 safety.
keywords: robotics, kinematics, inverse kinematics, dh parameters, jacobian, singularity, pid, feedforward, trajectory planning, motion planning, ros 2, moveit, ros2_control, iso 10218, iso/ts 15066, cobot
source: ROS 2 documentation - Distributions | https://github.com/ros2/ros2_documentation/blob/rolling/source/Releases.rst
source: ros2_control documentation | https://control.ros.org/rolling/index.html
source: ISO 10218-2:2025 Robotics - Safety requirements - Part 2 | https://www.iso.org/standard/73934.html
---
## When to use
Modelling an arm, solving IK, diagnosing jerky or unstable motion, tuning joint loops, planning paths, or checking a robot cell or cobot application for safety.

## Kinematics
- Classic DH per link: a_i (length along x_i), alpha_i (twist about x_i), d_i (offset along z_(i-1)), theta_i (angle about z_(i-1)); A_i = Rz(theta_i) Tz(d_i) Tx(a_i) Rx(alpha_i); T = A_1...A_n. State classic or modified (Craig) DH; mixing them is the top error. URDF uses joint origins (xyz, rpy).
- IK: closed form for 6R arms with a spherical wrist (wrist centre p_w = p - d6*R*z, then position and orientation separately); up to 8 solutions; choose by joint limits and nearest to current q. Numerical: damped least squares dq = J^T (J J^T + lambda^2 I)^-1 e.
- Jacobian: xdot = J(q) qdot; static torque tau = J^T F. Singularity where J loses rank; manipulability w = sqrt(det(J J^T)). 6-axis cases: wrist (theta5 = 0, axes 4 and 6 aligned), shoulder (wrist centre on axis 1), elbow (arm straight). Joint speeds blow up near them: slow down, use DLS, or re-plan.

## Control
- PID: u = Kp e + Ki integral(e) + Kd de/dt; derivative on measurement with a low-pass filter, anti-windup (clamping or back-calculation), saturation awareness.
- Cascade current -> velocity -> position, each inner loop about 5-10x the outer bandwidth; sample at 10-20x the closed-loop bandwidth.
- Feedforward does the work, feedback fixes the error: Kv*qdot_ref + Ka*qddot_ref, gravity compensation g(q), or computed torque tau = M(q)(qddot_d + Kd edot + Kp e) + C(q,qdot)qdot + g(q).
- Tuning targets: phase margin >= 45 deg, gain margin >= 6 dB; Ziegler-Nichols only as a start.

## Trajectories and planning
- Point to point: trapezoidal velocity (v_max, a_max) or jerk-limited S-curve; cubic (position and velocity) or quintic (plus acceleration) polynomials; synchronise joints to the slowest; time-parameterise (TOTG or Ruckig in MoveIt 2).
- Cartesian: linear position plus SLERP orientation; check IK continuity and singularities along the path.
- Planners: OMPL (RRT-Connect, PRM), CHOMP/STOMP, Pilz PTP/LIN/CIRC; FCL collision checking; Nav2 for mobile bases.

## ROS 2
- Distributions: Jazzy Jalisco (May 2024-May 2029), Kilted Kaiju (to Dec 2026), Lyrical Luth (May 2026-May 2031); Humble ends May 2027.
- ros2_control: controller manager loop read -> update -> write; system/actuator/sensor hardware interfaces expose state and command interfaces; joint_trajectory_controller.
- QoS: best effort for high-rate sensors, reliable for commands, set deadline and history depth. Real-time: PREEMPT_RT, no allocation, locks or logging in the loop, realtime_tools buffers, lifecycle nodes.

## Safety (ISO 10218-1/-2:2025, ISO/TS 15066:2016)
- Part 1 covers the robot (manufacturer), Part 2 the application and cell (integrator); risk assessment to ISO 12100; safety functions to ISO 13849-1 or IEC 62061 at the PL/SIL the assessment requires. The 2025 Part 2 absorbs much of the TS 15066 guidance (verify locally).
- Collaborative methods: safety-rated monitored stop, hand guiding, speed and separation monitoring (SSM), power and force limiting (PFL). Assess the application; a cobot alone is not safe.
- SSM: S_p = S_h + S_r + S_s + C + Z_d + Z_r, with S_h = v_h(T_r + T_s), v_h = 1.6 m/s unless justified, S_r = v_r*T_r, S_s = stopping distance, C = intrusion, Z = sensor and robot position uncertainty.
- PFL: body-region force and pressure limits (TS 15066 Annex A); transient contact may reach 2x quasi-static values; E = F^2/(2k) = 0.5*mu*v^2 with mu = (1/m_H + 1/m_R)^-1, so v_max = F_max/sqrt(mu*k). Verify by measurement at worst contact points.

## Pitfalls
Wrong DH convention; IK ignoring joint limits; integral windup; Cartesian moves through singularities; payload left out of PFL checks.
