# Janiform: Affine Hawkes–NIG with Coupled News Gaps
## Multi-scale tick generator with tractable coarse-time pricing/simulation

> **Purpose:** A tractable, calibratable model that generates **realistic financial price paths** (and optionally tapes) while matching stylized facts across time scales, and enabling **risk simulation** and **options pricing** without simulating billions of ticks.
>
> This document reconstructs the full brainstorming session: **all decisions**, **equations**, and **reasoning**. It should be sufficient for a quant dev to implement and for a researcher to draft a white paper.

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
- Must support **direct simulation and pricing at coarse time steps** (e.g., coarse-interval (e.g., hourly)) without tick simulation.
- Must avoid unrealistic assumptions like "ticks lie on a fixed millisecond grid."

---

## 1. Key terminology

- **Calendar time:** real clock time \(t\).
- **Business time:** a stochastic "activity clock" driven by trade intensity.
- **Projective / semigroup / scale consistency:** coarse-time transition laws are consistent with refined-time transitions (Chapman–Kolmogorov consistency). Practically: aggregating simulated fine-scale returns equals simulating directly at a coarser scale.
- **Market open/closed indicator:** \(\mathbf{1}_{\text{open}}(t)\), \(\mathbf{1}_{\text{closed}}(t)=1-\mathbf{1}_{\text{open}}(t)\).  

---

## 2. High-level architecture

### Scope note (explicit)
- **Tick-event generation is in scope and primary.** The canonical fine-scale model is the bivariate buy/sell trade point process with stochastic intensity and exogenous shocks.
- **Coarse-time pricing/simulation is derived for tractability.** For any chosen interval length \(\Delta\), we use transforms/sufficient-statistics to avoid generating all ticks when the use case is pricing or large-scale risk simulation.

We commit to (projective/semigroup consistency):  
> The model must admit an explicit coarse-time transition / transform so that **coarse-interval (e.g., hourly) simulation and option pricing** are feasible without tick simulation.

### 2.1 Price model at coarse scale
Total log-return process \(X_t\) is decomposed as:

\[
X_t = \underbrace{L^+_{A^+_t} - L^-_{A^-_t}}_{\text{trading-time moves}}
\; + \; \underbrace{M_t}_{\text{direct news jumps in open windows}}
\; + \; \underbrace{G_t}_{\text{gap jumps from closed-window news}}.
\]

Where:

- \(L^+\) and \(L^-\) are **NIG Lévy processes** (heavy tails + skew), run in **business time**.
- \(A^\pm_t = \int_0^t \lambda^\pm_u\,du\) are **integrated intensities** of buy/sell trade processes.
- \(M_t\) is the direct open-window news-jump component.
- \(G_t\) is the closed-window news-jump component realized as market-open gaps.
  **Clarification:** In the final specification below, \(M_t\) and \(G_t\) are both driven by the same aggregated news-arrival stream used for exogenous activity shocks, where the aggregated stream is the superposition of known-schedule and surprise-news components.

**Reasoning:**  
- Time-change by integrated intensity preserves duration information (silences/bursts matter).
- NIG Lévy increments give heavy tails; aggregation "thins" tails naturally via mixing and summing.
- Separating open-window direct jumps from closed-window gap jumps preserves market-state semantics.
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
For pricing/sufficient-statistics per interval:

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
- Shocks while open should create immediate direct return jumps and also feed Hawkes activity excitation

### 5.1 Two-component shared news process with dependent marks
News arrivals are modeled as a superposition:
\[
N_t^{\text{news}} = N_t^{\text{known}} + N_t^{\text{surprise}},
\]
where:
- \(N_t^{\text{known}}\) represents known-in-advance scheduled-event times (shock realization is still random).
- \(N_t^{\text{surprise}}\) captures unscheduled/out-of-nowhere shocks.

The aggregated news intensity is
\[
\nu_{\text{news}}(t)=\nu_{\text{known}}(t)+\nu_{\text{surprise}}(t).
\]

**Final decision:** news arrivals are allowed in both open and closed windows, while direct gap increments remain closed-only through the \(G_t\) gating rule in Section 5.3.
Trade-arrival seasonality \(s(t)\) applies only to \(\lambda^\pm_t\), not to \(\nu_{\text{news}}(t)\).

For implementation, use a deterministic scheduled-event component plus a smooth background component:
\[
\nu_{\text{surprise}}(t)=\exp\Big(g_{\text{dow}}(\mathrm{dow}(t)) + g_{\text{doy}}(\mathrm{doy}(t)) + g_{\text{holiday}}(t) + g_{\text{session}}(\mathrm{session}(t))\Big),
\]
with \(\mathrm{session}(t)\in\{\text{open},\text{closed}\}\), and \(\nu_{\text{known}}(t)\) built from the known event calendar.

### 5.2 Dependent coupling between "news jump" and "activity burst" (closed-form)
At each news time \(\tau_m\) from either component (\(N^{\text{known}}\) or \(N^{\text{surprise}}\)), draw severity and gap mark:

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

3. News-jump mark:
\[
Z_m = \sigma_G \sqrt{U_m}\,\varepsilon_m.
\]

**Decision:** Keep \(p\) constant (no extra skew in gaps).  
**Reasoning:** Slight negative gap bias exists empirically, but not worth DOF here; skew already modeled elsewhere.

### 5.3 Processes \(J_t\), \(M_t\), and \(G_t\)
Define:

- Exogenous Hawkes shock accumulator:
\[
J_t^{\text{known}} = \sum_{\tau_m^{\text{known}} \le t} U_m^{\text{known}},
\qquad
J_t^{\text{surprise}} = \sum_{\tau_m^{\text{surprise}} \le t} U_m^{\text{surprise}},
\]
\[
J_t = J_t^{\text{known}} + J_t^{\text{surprise}}.
\]

- Gap return component (closed only):
\[
G_t^{\text{known}} = \sum_{\tau_m^{\text{known}} \le t} \mathbf{1}_{\text{closed}}(\tau_m^{\text{known}})\, Z_m^{\text{known}},
\qquad
G_t^{\text{surprise}} = \sum_{\tau_m^{\text{surprise}} \le t} \mathbf{1}_{\text{closed}}(\tau_m^{\text{surprise}})\, Z_m^{\text{surprise}},
\]
\[
G_t = G_t^{\text{known}} + G_t^{\text{surprise}}.
\]

- Direct open-window news-jump component:
\[
M_t^{\text{known}} = \sum_{\tau_m^{\text{known}} \le t} \mathbf{1}_{\text{open}}(\tau_m^{\text{known}})\, Z_m^{\text{known}},
\qquad
M_t^{\text{surprise}} = \sum_{\tau_m^{\text{surprise}} \le t} \mathbf{1}_{\text{open}}(\tau_m^{\text{surprise}})\, Z_m^{\text{surprise}},
\]
\[
M_t = M_t^{\text{known}} + M_t^{\text{surprise}}.
\]

**Decision:** News shocks always feed \(J_t\) and therefore Hawkes activity excitation. Open-window news applies direct return jumps through \(M_t\) on the first post-arrival open tick. Closed-window news applies direct return jumps through \(G_t\) on the first reopening tick as a market-open gap.

### 5.4 Calibration strategy for trade and news layers
Inputs required:
- Trade tick timestamps with buy or sell sign labels.
- Exchange open and close calendar windows.
- Known scheduled-news timestamps (event calendar).
- Return series at tick and close-open boundaries.

Recommended staged fit:
1. Trade seasonality fit:
   Estimate \(s(t)\) from trade-arrival timestamps only, using a continuous step function with hourly steps.
   Normalize so \(\mathbb E[s(t)]\approx 1\) on the training window.
2. Deseasonalized Hawkes fit:
   Build deseasonalized intensities from \(\tilde\lambda_t^\pm=\lambda_t^\pm/s(t)\) and fit Hawkes parameters \((\bar\lambda,\beta,\alpha_{ij})\) on trade arrivals.
   This step captures endogenous self and cross excitation without leaking deterministic calendar structure into Hawkes parameters.
3. Known-news arrival component:
   Treat \(N_t^{\text{known}}\) as exogenous from the provided event calendar.
   Estimate only shock-response and mark parameters for these arrivals, not their event times.
4. Surprise-news arrival component:
   Infer candidate surprise-news times from residual return jumps and close-open gaps that are not explained by known-news times.
   Fit \(\nu_{\text{surprise}}(t)\) with calendar covariates \((\mathrm{dow},\mathrm{doy},\mathrm{holiday},\mathrm{session})\) using a point-process likelihood on inferred arrivals.
5. News-jump mark fit:
   Fit \((k_U,\theta_U,\sigma_G,p,\eta_+,\eta_-)\) from jump magnitudes using:
   - open-window first-tick jumps for \(M_t\),
   - close-open gaps for \(G_t\),
   - known-news event windows and inferred surprise-news event windows.
   Keep one shared mark law unless diagnostics justify a known versus surprise split.
6. News-to-Hawkes coupling fit:
   Fit \(\rho_{ij}\) from post-shock activity response, enforcing the structured constraints from Section 6.2.
   This guarantees news shocks increase activity through \(J_t\) even when their direct return effect is in \(M_t\) or \(G_t\).
7. Joint refinement:
   Run a constrained joint optimization over Hawkes plus news parameters with deterministic seeds, preserving:
   - positivity and stability constraints,
   - open-window direct-jump semantics for \(M_t\),
   - closed-window gap semantics for \(G_t\).

Practical identifiability notes:
- Keep \(s(t)\) estimation isolated from news estimation to avoid confounding deterministic trade seasonality with news arrivals.
- Anchor \(G_t\) primarily on close-open returns and \(M_t\) primarily on open-window first-tick jump returns.
- Use known-news timestamps as hard anchors before fitting surprise arrivals.

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
\Big)\cdot \varphi_{\Delta M+\Delta G}(u).
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
Computes coarse-interval (e.g., hourly) CF of returns conditional on hour boundary state \(\mathbf Y_h\), then prices via Fourier methods.

### 9.2 Engine B: Risk simulation (sufficient-statistics sampler)
Samples sufficient statistics \((A^+,A^-,N^+,N^-)\) per interval and updates \(\mathbf Y\) at hour boundaries; then samples NIG increments conditional on \(A^\pm\), applies direct open-window news jumps \(M\), and applies closed-window gap jumps \(G\).

**Final decision:** Use **sufficient statistics** (not cluster simulation).

---

## 10. Sufficient-statistics oracle (explicit ODE system)

Define transform over one interval \([0,\Delta]\) with piecewise constant \(s(t)=s_h\), \(\nu_{\text{news}}(t)=\nu_h\), where \(\nu_h=\nu^{\text{known}}_h+\nu^{\text{surprise}}_h\):

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
