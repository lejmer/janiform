# Affine Multi-Scale Tick-Consistent Market Generator (Brainstorm Summary)

> **Purpose:** A tractable, calibratable model that generates **realistic financial price paths** (and optionally tapes) while matching stylized facts across time scales, and enabling **risk simulation** and **options pricing** without simulating billions of ticks.
>
> This document reconstructs the full brainstorming session: **all decisions**, **equations**, and **reasoning**. It should be sufficient for a quant dev to implement and for a researcher to draft a white paper.
>
> Proposed model name: "Janiform: Affine Hawkes–NIG model with coupled news gaps"

---

## 0. Design goals and constraints

### Required empirical properties (targeted)
1. **Business clock / random event time**  
   Trade arrivals are stochastic with **time-varying intensity** driven by:
   - time-of-day effects (not necessarily U-shaped)
   - day-of-week effects (non-optional)
   - day-of-year / holiday / scheduled-event effects
   - long-run changes and short-run shocks
   - **self-excitation** (bursty activity)

2. **Heavy tails and negative skewness**  
   Returns display:
   - strong heavy tails at tick level
   - negative skewness (usually mild but persistent across assets)
   - tails become less extreme under aggregation (kurtosis decreases with horizon)

3. **Volatility clustering**  
   Strong persistence of volatility, correlated with activity clustering.

4. **Jumps / regime shocks**  
   - "Out-of-nowhere" shocks (news) that ignite activity/volatility
   - For closed markets: **overnight/weekend gap moves** + turbulent re-open dynamics

5. **Time-scale consistency**  
   Aggregation consistency across horizons: generating 1h returns directly should match aggregating tick-level returns implied by the same model structure.

### Tractability requirements (hard constraints)
- Must be calibratable in reasonable time.
- Must support **direct simulation and pricing at coarse time steps** (e.g., hourly) without tick simulation.
- Must avoid unrealistic assumptions like "ticks lie on a fixed millisecond grid."

---

## 1. Key terminology

- **Calendar time:** real clock time \(t\).
- **Business time:** a stochastic "activity clock" driven by trade intensity.
- **Projective / semigroup / scale consistency:** coarse-time transition laws are consistent with refined-time transitions (Chapman–Kolmogorov consistency). Practically: aggregating simulated fine-scale returns equals simulating directly at a coarser scale.
- **Market open/closed indicator:** \(\mathbf{1}_{\text{open}}(t)\), \(\mathbf{1}_{\text{closed}}(t)=1-\mathbf{1}_{\text{open}}(t)\).  
  **Convention:** "closed" includes overnight, weekend, and exchange holidays. If a simulation step straddles an open/close boundary, either (i) split the step at the boundary or (ii) align the coarse grid to session boundaries (recommended for 1h stepping).

---

## 2. High-level architecture

We commit to projective/semigroup consistency:
> The model must admit an explicit coarse-time transition / transform so that **hourly simulation and option pricing** are feasible without tick simulation.

### 2.1 Price model at coarse scale
Total log-return process \(X_t\) is decomposed as:

\[
X_t = \underbrace{L^+_{A^+_t} - L^-_{A^-_t}}_{\text{trading-time moves}} \; + \; \underbrace{G_t}_{\text{gap moves during closures}}.
\]

Where:

- \(L^+\) and \(L^-\) are **NIG Lévy processes** (heavy tails + skew), run in **business time**.
- \(A^\pm_t = \int_0^t \lambda^\pm_u\,du\) are **integrated intensities** of buy/sell trade processes.
- \(G_t\) is a **compound Poisson** jump process that only produces returns when the market is closed (overnight/weekend/holiday), with **Kou / double-exponential** jumps.  
  **Clarification:** In the final specification below, \(G_t\) is driven by the *same* Poisson news arrivals as the exogenous activity shocks; it is therefore compound Poisson, but not an independent jump process.

**Reasoning:**  
- Time-change by integrated intensity preserves duration information (silences/bursts matter).
- NIG Lévy increments give heavy tails; aggregation "thins" tails naturally via mixing and summing.
- Separating a gap component allows realistic weekend/overnight jumps without trading during closure.
- Using bivariate buy/sell intensities preserves sign dynamics and skew.

---

## 3. Trade arrival model (bivariate Hawkes in calendar time)

### 3.1 Two counting processes: buys and sells
\[
N_t^+,\; N_t^- \quad \text{with intensities}\quad \lambda_t^+,\; \lambda_t^-.
\]

We model the **de-seasonalized** intensities as:

\[
\lambda_t^\pm = s(t)\,\tilde\lambda_t^\pm.
\]

- \(s(t)\) is deterministic seasonality (Section 4).
- \(\tilde\lambda_t^\pm\) is a bivariate self-exciting process (Hawkes shot-noise, exponential kernel).

### 3.2 Hawkes excitation state (4D)
Define excitation components \(Y_t^{ij}\) with \(i\in\{+,-\}\) (target intensity) and \(j\in\{+,-\}\) (source event sign).

\[
\tilde\lambda_t^+ = \bar\lambda + Y_t^{++} + Y_t^{+-},
\qquad
\tilde\lambda_t^- = \bar\lambda + Y_t^{-+} + Y_t^{--}.
\]

**Decision:**
- Baselines equal: \(\bar\lambda^+=\bar\lambda^-=\bar\lambda\).

**Reasoning:**  
A constant sell-bias in baseline counts would imply persistent down-drift unless counterbalanced; we want skew without forcing long-term decline.

### 3.3 Exponential-kernel dynamics (shared decay \(\beta\))
Shared decay was chosen for identifiability and tractability:

\[
dY_t^{ij} = -\beta Y_t^{ij}\,dt \; + \; \alpha_{ij}\, dN_t^{j} \; + \; \rho_{ij}\, dJ_t,
\qquad i,j\in\{+,-\}.
\]

- \(\beta>0\) is **shared** across all \(ij\).  
- \(\alpha_{ij}\ge 0\) govern endogenous self/cross excitation.
- \(\rho_{ij}\ge 0\) load common exogenous news-shock severity \(J_t\) into the Hawkes state.

**Reasoning:**  
- Exponential kernels make the system **finite-dimensional Markov**, enabling transforms for \(\int \lambda\,dt\).
- Shared \(\beta\) reduces DOF with minimal loss of realism and better calibration stability.

### 3.4 Positivity and stability constraints (implementation guardrails)
- Enforce \(\bar\lambda>0\), \(\alpha_{ij}\ge 0\), \(\rho_{ij}\ge 0\), \(\beta>0\).
- Hawkes stability for the de-seasonalized bivariate kernel (exponential with shared \(\beta\)): define the \(2\times 2\) matrix
  \[
  \Alpha=
  \begin{pmatrix}
  \alpha_{++} & \alpha_{+-}\\
  \alpha_{-+} & \alpha_{--}
  \end{pmatrix}.
  \]
  A sufficient (and standard) stationarity condition is
  \[
  \rho(\Alpha/\beta)<1,
  \]
  where \(\rho(\cdot)\) is the spectral radius. This prevents explosive self-excitation in calibration/simulation.

---

## 4. Deterministic seasonality \(s(t)\)

### 4.1 General factorization (no U-shape assumption)
We avoid parametric intraday shapes; instead:

\[
s(t)=\exp\Big( f_{\text{intra}}(\mathrm{tod}(t)) \; + \; f_{\text{dow}}(\mathrm{dow}(t)) \; + \; f_{\text{doy}}(\mathrm{doy}(t)) \; + \; f_{\text{special}}(t)\Big).
\]

- \(f_{\text{intra}}\): arbitrary smooth function over time-of-day (Fourier/splines/circular GP).
- \(f_{\text{dow}}\): day-of-week effects (non-optional).
- \(f_{\text{doy}}\): seasonal/holiday structure.
- \(f_{\text{special}}(t)\): scheduled events (auctions, macro releases, etc.), if desired.

### 4.2 Estimation strategy (chosen)
Estimate \(s(t)\) nonparametrically from timestamps alone (or weakly assisted by volume/spread), then **freeze** for joint calibration.

**Reasoning:**  
Pins down predictable calendar structure and prevents confounding with latent stress/excitation dynamics, improving identifiability.

**Implementation note (identifiability):** Normalize \(s(t)\) so it does not absorb the overall event rate (e.g., enforce \(\mathbb E[s(t)]=1\) over the estimation window, or equivalently center the fitted log-seasonality). This avoids confounding with \(\bar\lambda\).

### 4.3 Piecewise constant approximation (final)
For pricing/sufficient-statistics per hour:

\[
s(t)\approx s_h \quad \text{for } t\in[h,h+1\text{h}).
\]

**Reasoning:**  
This yields a homogeneous-in-hour Hawkes structure, greatly simplifying transforms with minimal approximation error at the 1h step.

---

## 5. Exogenous news shocks and closed-market gaps

We need:
- Out-of-nowhere shocks that ignite activity/volatility
- Weekend/overnight gap moves for closed markets
- Shocks while open should be handled primarily by Hawkes state (no separate price jump needed)

### 5.1 One shared "news process" with dependent marks
News arrivals: a Poisson process \(N_t^{\text{news}}\) with deterministic intensity \(\nu_{\text{news}}(t)\).

**Final decision:** \(\nu_{\text{news}}(t)\) must be active during *both* open and closed windows to allow open-market exogenous shocks (activity bursts), while **gap returns** are still applied only when closed.

A convenient two-regime deterministic specification is:

\[
\nu_{\text{news}}(t)
=
\mathbf{1}_{\text{open}}(t)\,\exp\Big(g_{\text{open,dow}}(\mathrm{dow}(t)) + g_{\text{open,doy}}(\mathrm{doy}(t)) + g_{\text{open,holiday}}(t)\Big)
\;+\;
\mathbf{1}_{\text{closed}}(t)\,\exp\Big(g_{\text{closed,dow}}(\mathrm{dow}(t)) + g_{\text{closed,doy}}(\mathrm{doy}(t)) + g_{\text{closed,holiday}}(t)\Big).
\]

### 5.2 Dependent coupling between "gap jump" and "activity burst" (closed-form)
At each news time \(\tau_m\), draw severity and gap mark:

1. Severity:
\[
U_m \sim \Gamma(k_U,\theta_U),
\quad U_m \ge 0.
\]

2. Kou / double-exponential base shock \(\varepsilon_m\):
\[
f_{\varepsilon}(x)=
\begin{cases}
p\,\eta_+ e^{-\eta_+ x}, & x\ge 0,\\
(1-p)\,\eta_- e^{\eta_- x}, & x<0,
\end{cases}
\quad \eta_+,\eta_->0,\; p\in(0,1).
\]

3. Gap jump size:
\[
Z_m = \sigma_G \sqrt{U_m}\,\varepsilon_m.
\]

**Decision:** Keep \(p\) constant (no extra skew in gaps).  
**Reasoning:** Slight negative gap bias exists empirically, but not worth DOF here; skew already modeled elsewhere.

### 5.3 Processes \(J_t\) and \(G_t\)
Define:

- Exogenous Hawkes shock accumulator:
\[
J_t = \sum_{\tau_m \le t} U_m.
\]
  **Clarification:** \(J_t\) is a cumulative marked-sum process (severity accumulator), not a separate price-jump process.

- Gap return component (closed only):
\[
G_t = \sum_{\tau_m \le t} \mathbf{1}_{\text{closed}}(\tau_m)\, Z_m.
\]

**Decision:** When news hits during open, Hawkes-state dynamics dominate; gap returns apply only when closed.  
**Clarification:** Open-news affects prices *indirectly* via higher activity (\(A^\pm\)) and therefore larger/heavier trading-time increments; closed-news additionally contributes an explicit gap return \(G\).

---

## 6. Endogenous asymmetry decisions (\(\alpha\), \(\rho\))

### 6.1 Endogenous excitation \(\alpha_{ij}\) (structured DOF)
We enforce inequalities
\(\alpha_{--}>\alpha_{++}\) and \(\alpha_{-+}>\alpha_{+-}\),
using a base + deltas parameterization:

Let \(a_{\text{self}}>0\), \(a_{\text{cross}}\ge 0\),
\(\Delta_{\text{self}}\ge 0\), \(\Delta_{\text{cross}}\ge 0\).

\[
\alpha_{++}=a_{\text{self}},\qquad
\alpha_{--}=a_{\text{self}}+\Delta_{\text{self}},
\]
\[
\alpha_{+-}=a_{\text{cross}},\qquad
\alpha_{-+}=a_{\text{cross}}+\Delta_{\text{cross}}.
\]

**Reasoning:**  
- More sell-side self-excitation matches "panic clustering."
- Cross asymmetry matches "fragility" (buy activity triggers sell cascades more than vice versa).
- Structured DOF improves identifiability and robustness.

### 6.2 Exogenous news loadings \(\rho_{ij}\) (risk-relevant; not collapsed)
We **do not** set \(\rho_{ij}=\rho\) because it can blunt crash-risk dynamics relevant for pricing/risk.

We use the same structured scheme, with self-dominant loading:

Let \(r_{\text{self}}>0\), \(r_{\text{cross}}\ge 0\),
\(\Delta^{(r)}_{\text{self}}\ge 0\), \(\Delta^{(r)}_{\text{cross}}\ge 0\).

\[
\rho_{++}=r_{\text{self}},\qquad
\rho_{--}=r_{\text{self}}+\Delta^{(r)}_{\text{self}},
\]
\[
\rho_{+-}=r_{\text{cross}},\qquad
\rho_{-+}=r_{\text{cross}}+\Delta^{(r)}_{\text{cross}}.
\]

**Decision:** "News primarily amplifies self-excitation"  
Enforce \(r_{\text{self}} \gg r_{\text{cross}}\) (e.g., \(r_{\text{cross}}=\epsilon r_{\text{self}}\) with small \(\epsilon\)).

---

## 7. Heavy-tailed returns via NIG Lévy processes in business time

### 7.1 NIG choice and reasoning
We chose NIG because it is tractable, heavy-tailed, skew-capable, and supports CF-based pricing.

Tail "thinning" under aggregation emerges naturally via random time-change and summation over longer horizons.

### 7.2 Buy/sell Lévy drivers with 1-parameter asymmetry
Let the NIG exponent be \(\psi_{\text{NIG}}(u;\alpha,\beta,\delta,\mu)\) so that:
\[
\mathbb{E}\left[e^{iu L_t}\right] = \exp\left(t\,\psi_{\text{NIG}}(u;\alpha,\beta,\delta,\mu)\right),
\]
with \(\alpha>|\beta|\), \(\delta>0\).

**Decision:** Shared core parameters and single skew shift:
- Buy: \((\alpha,\beta_0,\delta,\mu)\)
- Sell: \((\alpha,\beta_0+\Delta_\beta,\delta,\mu)\) with \(\Delta_\beta<0\).

Conditional CF given \((A^+,A^-)\):
\[
\mathbb E\!\left[e^{iu(X_{t+\Delta}-X_t)}\mid A^+,A^-\right]
=
\exp\Big(
A^+\,\psi_{\text{NIG}}(u;\alpha,\beta_0,\delta,\mu)
+
A^-\,\psi_{\text{NIG}}(-u;\alpha,\beta_0+\Delta_\beta,\delta,\mu)
\Big)\cdot \varphi_{\Delta G}(u).
\]

**Clarification:** Because \(X\) subtracts the sell-side driver \(L^-_{A^-}\), its characteristic function contributes \(\exp\big(A^-\,\psi_-(-u)\big)\) rather than \(\exp\big(A^-\,\psi_-(u)\big)\).

---

## 8. Risk-neutral measure and pricing philosophy (final)

**Decision:** Use an **Esscher tilt** for the Lévy part under \(Q\), while keeping Hawkes structure under \(Q\) unchanged (optional tilt of exogenous news intensity if later needed).

**Caveat / modeling stance:** Keeping the Hawkes/intensity dynamics invariant under \(Q\) is a parsimonious "minimal-change" pricing measure choice; empirically, activity/volatility can carry a risk premium. If required by calibration to option smiles, one can extend the model by tilting \(\nu_{\text{news}}\) (or exogenous loading parameters) under \(Q\) while preserving tractability.

For Lévy parts, Esscher tilt \(\theta\) yields:
\[
\psi_Q(u) = \psi_P(u+i\theta) - \psi_P(i\theta).
\]

Martingale condition under \(Q\):
\[
\mathbb E^Q[e^{X_{t+\Delta}-X_t}\mid \mathcal F_t] = e^{(r-q)\Delta}.
\]

---

## 9. Hybrid implementation strategy (final)

### 9.1 Engine A: Pricing (CF engine)
Computes hourly CF of returns conditional on hour boundary state \(\mathbf Y_h\), then prices via Fourier methods.

### 9.2 Engine B: Risk simulation (sufficient-statistics sampler)
Samples sufficient statistics \((A^+,A^-,N^+,N^-)\) per hour and updates \(\mathbf Y\) at hour boundaries; then samples NIG increments conditional on \(A^\pm\) and adds gap component if closed.

**Final decision:** Use **sufficient statistics** (not cluster simulation).

---

## 10. Sufficient-statistics oracle (explicit ODE system)

Define transform over one hour \([0,\Delta]\) with piecewise constant \(s(t)=s_h\), \(\nu_{\text{news}}(t)=\nu_h\):

\[
F(u_+,u_-,z_+,z_- \mid \mathbf Y_0)
=
\mathbb E\Big[
e^{-u_+ A^+ -u_- A^-}\;
z_+^{N^+}\; z_-^{N^-}
\;\Big|\; \mathbf Y_0\Big],
\]
where
\[
A^\pm=\int_0^\Delta \lambda_t^\pm dt,\qquad
N^\pm = N^\pm_\Delta-N^\pm_0.
\]

### 10.1 Intensities in the hour
\[
\lambda_t^+ = s_h(\bar\lambda + Y_t^{++}+Y_t^{+-}),\qquad
\lambda_t^- = s_h(\bar\lambda + Y_t^{-+}+Y_t^{--}).
\]

State vector:
\[
\mathbf y_t=(Y_t^{++},Y_t^{+-},Y_t^{-+},Y_t^{--}).
\]

### 10.2 Jump impact vectors
Buy event:
\[
\mathbf k_+ = (\alpha_{++},\,0,\,\alpha_{-+},\,0).
\]
Sell event:
\[
\mathbf k_- = (0,\,\alpha_{+-},\,0,\,\alpha_{--}).
\]
News loading vector:
\[
\mathbf r = (\rho_{++},\,\rho_{+-},\,\rho_{-+},\,\rho_{--}).
\]

### 10.3 Exponential–affine ansatz
Assume
\[
f(\tau,\mathbf y)
=
\mathbb E_{\mathbf y_0=\mathbf y}\!\left[
e^{-u_+\!\int_0^\tau \lambda_t^+dt-u_-\!\int_0^\tau \lambda_t^-dt}\; z_+^{N^+_\tau}\; z_-^{N^-_\tau}
\right]
= \exp\big(C(\tau)+\mathbf D(\tau)\cdot \mathbf y\big),
\]
with
\[
C(0)=0,\qquad \mathbf D(0)=\mathbf 0.
\]

### 10.4 Hamiltonians
\[
H_+(\mathbf D)= z_+ \exp(\mathbf D\cdot\mathbf k_+) - 1 - u_+,
\qquad
H_-(\mathbf D)= z_- \exp(\mathbf D\cdot\mathbf k_-) - 1 - u_-.
\]

Assume \(U\sim\Gamma(k_U,\theta_U)\):
\[
\mathbb E[e^{sU}] = (1-\theta_U s)^{-k_U}\qquad \text{for } s<1/\theta_U.
\]

### 10.5 ODE system
For \(\tau\in[0,\Delta]\):

\[
\begin{aligned}
D_{++}'(\tau) &= -\beta D_{++}(\tau) + s_h\,H_+(\mathbf D(\tau)),\\
D_{+-}'(\tau) &= -\beta D_{+-}(\tau) + s_h\,H_+(\mathbf D(\tau)),\\
D_{-+}'(\tau) &= -\beta D_{-+}(\tau) + s_h\,H_-(\mathbf D(\tau)),\\
D_{--}'(\tau) &= -\beta D_{--}(\tau) + s_h\,H_-(\mathbf D(\tau)).
\end{aligned}
\]

\[
C'(\tau)
= s_h\,\bar\lambda\big(H_+(\mathbf D(\tau)) + H_-(\mathbf D(\tau))\big)
\; + \;
\nu_h\Big(\big(1-\theta_U\,(\mathbf D(\tau)\cdot \mathbf r)\big)^{-k_U}-1\Big).
\]

**Domain note:** The news term requires \(\mathbf D(\tau)\cdot\mathbf r < 1/\theta_U\) for all \(\tau\in[0,\Delta]\). Numerically, this provides a natural blow-up/guard condition during ODE integration.

Finally,
\[
F(u_+,u_-,z_+,z_- \mid \mathbf Y_0)=\exp\big(C(\Delta)+\mathbf D(\Delta)\cdot \mathbf y_0\big).
\]

### 10.6 Oracle usage
**Pricing CF (trading-time component):**
\[
\varphi_{\text{trade}}(u\mid \mathbf Y_0)
=
F\big(u_+=-\psi_+(u),\; u_-=-\psi_-(-u),\; z_+=1,\; z_-=1 \mid \mathbf Y_0\big),
\]
with
\[
\psi_+(u)=\psi_{\text{NIG}}(u;\alpha,\beta_0,\delta,\mu),\quad
\psi_-(u)=\psi_{\text{NIG}}(u;\alpha,\beta_0+\Delta_\beta,\delta,\mu).
\]

Risk sampling uses slices of \(F\) to sample \((N^+,N^-)\) and \((A^+,A^-)\) without simulating clusters/ticks.

---

## 11. Explicit omissions (by decision)
- Full LOB events (adds/cancels) — trades-only selected for tractability.
- High-fidelity tick volume dynamics — volume is secondary; can be added as an auxiliary mark later.

---

## 12. State carried between hours
We retain full 4D excitation state:
\[
\mathbf Y_h = (Y_h^{++},Y_h^{+-},Y_h^{-+},Y_h^{--}).
\]
