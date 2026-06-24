## 1. What to include

| Topic                             | Include                                                                                                             |
|-----------------------------------|---------------------------------------------------------------------------------------------------------------------|
| Motivation                        | Why a unified tick/coarse/pricing model is useful                                                                   |
| Existing model families           | Hawkes, LOB queue models, agent-based simulators, stochastic/rough vol, Lévy/subordinated models, neural generators |
| Main contribution                 | Dual directional activity clocks, dual business-time drivers, shared sufficient statistics, affine pricing bridge   |
| Core return process               | \(X_t^{\mathrm{core}}=\widetilde L^+_{A_t^+}-\widetilde L^-_{A_t^-}+M_t+G_t\)                                       |
| Activity/event layer              | Open-market buy/sell intensities, seasonality, Hawkes excitation, structured asymmetry                              |
| News/gap layer                    | Known scheduled news, surprise news, shared severity, activity shock, open jumps, closed-session gaps               |
| Long-variance layer               | (Z_t), event feedback, unit stationary normalization, baseline return-clock scaling                                 |
| Business-time return drivers      | NIG side drivers, mean compensation, side skew shift                                                                |
| Observed-state mean reversion     | `X=A+D`, temporary deviation relaxation, and market-pressure anchor-pressure driven by `Omega`                      |
| Affine schedule transform         | (C,D,V,W) solver, schedule composition, domain constraints                                                          |
| Pricing layer                     | Terminal cumulant, activity-risk premiums (\pi_+,\pi_-), derived martingale tilt (\theta), risk-neutral CF          |
| Sufficient-statistics consistency | Seeded ticks, coarse simulation, sufficient-stat sampling, and affine pricing using the same decomposition          |
| Observation layer                 | Latent log-price to observed tick-grid projection                                                                   |
| Calibration                       | Staged fitting, constraints, regularization, identifiability                                                        |
| Validation                        | Real-vs-sim, aggregation, ablations, pricing tests                                                                  |
| Limitations                       | Compute cost, calibration complexity, domain constraints, model risk                                                |

## 2. What to compute / plot

| Category                  | Compute / plot                                                                                                           |
|---------------------------|--------------------------------------------------------------------------------------------------------------------------|
| Tick realism              | Inter-arrival distribution, event-count bursts, zero-return frequency                                                    |
| Activity                  | Real vs fitted intraday activity curve; buy/sell event intensities                                                       |
| Seasonality               | (s(t)) by time-of-day, weekday, day-of-year, special days                                                                |
| Hawkes behavior           | Excitation decay examples; branching ratio / spectral-radius diagnostics                                                 |
| News/gaps                 | Known vs surprise arrival counts; open-news jump examples; closed-news gap examples                                      |
| Shared severity           | Scatter/relationship between news severity, activity shock, and return jump                                              |
| Long variance             | Simulated (Z_t); baseline clock with/without (Z_t); (Z_t) autocorrelation                                                |
| Return distribution       | Histogram/KDE/QQ at tick, 1s, 1m, 5m, 1h, daily                                                                          |
| Tails                     | Tail plot, excess kurtosis by horizon, optional Hill estimator                                                           |
| Vol clustering            | ACF of (r), (r^2), realized variance                                                                                     |
| Directional asymmetry     | Signed-return stats conditional on buy/sell dominance; skew by horizon                                                   |
| Observed-state relaxation | Response/decay plot for (\kappa=0), weak (\kappa), strong (\kappa) showing temporary deviation relaxation                |
| Market pressure           | finite-state `Omega` path, signed-balance side composition, and anchor-pressure movement without total-activity feedback |
| Tick-grid layer           | Latent vs observed rounded path; distribution of observed tick moves                                                     |
| Aggregation               | Seeded tick aggregation vs direct coarse simulation                                                                      |
| Sufficient statistics     | Compare distributions/moments of ((B_Z^+,E^+,B_Z^-,E^-,N^+,N^-,Z_T)) from tick aggregation vs coarse sampler             |
| Pricing                   | IV smile/surface; pricing error by moneyness/tenor                                                                       |
| Martingale check          | Residual of martingale equation after solving (\theta)                                                                   |
| Parity check              | Put/call parity residuals                                                                                                |
| Pricing domain            | Validity/failure map over ((\pi_+,\pi_-))                                                                                |
| Calibration stability     | Parameter distributions across days, seeds, windows                                                                      |

Minimum v1 ablations:

| Ablation                                     | Purpose                                      |
|----------------------------------------------|----------------------------------------------|
| Single clock vs dual clocks                  | Value of directional activity                |
| Symmetric vs asymmetric excitation           | Value of structured asymmetry                |
| No Hawkes vs Hawkes                          | Value of endogenous bursts                   |
| No (Z_t) vs (Z_t)                            | Value of persistent long-variance memory     |
| No news/gaps vs news/gaps                    | Value of event/gap layer                     |
| Observed-state mean reversion off vs on      | Show optional temporary-deviation relaxation |
| Market pressure anchor-pressure off vs on    | Show `Omega` anchor-pressure contribution    |
| Tick aggregation vs direct coarse simulation | Core consistency test                        |
| Underlying-only vs joint option refinement   | Pricing-layer contribution                   |

## 3. What to derive equations for

| Derivation                                                    |  Priority |
|---------------------------------------------------------------|----------:|
| Conditional CF/MGF of trading block                           | Very high |
| Mean/variance conditional on activity clocks                  |      High |
| Side mean compensation and zero conditional drift             |      High |
| Hawkes stationary intensity and stability condition           |      High |
| Long-variance unit-mean normalization                         | Very high |
| First-moment guard for (Z_t)                                  | Very high |
| Conditional mean baseline clock scale (\overline Z_h)         |      High |
| News mark MGF (M_c(q,r))                                      |      High |
| Open-news vs closed-gap decomposition                         |      High |
| Observed-state `X=A+D` and market-pressure anchor pressure    |    Medium |
| Affine transform ansatz (F=\exp(C+D\cdot y_0+VZ_0+W\Omega_0)) | Very high |
| Riccati/ODE system for (C,D,V,W)                              | Very high |
| Affine domain constraints                                     | Very high |
| Terminal cumulant adapter (v\mapsto u_\pm^B,u_\pm^E,z_\pm,q)  | Very high |
| Pricing density normalization                                 | Very high |
| Martingale tilt equation for (\theta)                         | Very high |
| Risk-neutral characteristic function (\phi_Q(u))              | Very high |
| Put/call parity expression                                    |      High |
| Sufficient-statistics consistency                             | Very high |
| Tick-grid projection equation                                 |    Medium |

Most important derivations for the paper body:

1. Conditional CF/MGF.
2. Long-variance normalization.
3. Affine schedule transform.
4. Terminal pricing measure.
5. Sufficient-statistics consistency.

Everything else can be appendix-level.

## 4. Usual structure

Recommended structure:

1. Abstract
2. Introduction
3. Modeling requirements
4. Related work
5. Janiform overview
6. Core return process
7. Event activity, seasonality, and Hawkes excitation
8. News, gaps, and shared severity
9. Long-variance state and return clocks
10. Business-time return drivers
11. Observed-state deviation and market-pressure anchor pressure
12. Affine schedule transform
13. Sufficient-statistics consistency
14. Terminal affine pricing measure
15. Calibration strategy
16. Simulation and aggregation validation
17. Pricing validation
18. Ablation studies
19. Limitations
20. Conclusion

Appendices:

| Appendix | Content                               |
|----------|---------------------------------------|
| A        | Notation table                        |
| B        | Full compact model spec               |
| C        | CF/MGF derivations                    |
| D        | Affine transform derivation           |
| E        | Pricing derivations                   |
| F        | Calibration details                   |
| G        | Extra validation plots                |
| H        | Reproducibility: data, seeds, configs |

## 5. Relevant research / links

| Area                          | Reference                                                                                                                                       |
|-------------------------------|-------------------------------------------------------------------------------------------------------------------------------------------------|
| Stylized facts                | Cont, “Empirical properties of asset returns,” classic reference for heavy tails, volatility clustering, nonlinear dependence, etc. ([Rama][1]) |
| Modern stylized facts         | “Revisiting Cont’s Stylized Facts for Modern Stock Markets.” ([arXiv][2])                                                                       |
| Business time / subordination | Clark, “A Subordinated Stochastic Process Model with Finite Variance for Speculative Prices.” ([JSTOR][3])                                      |
| Hawkes in finance             | Bacry, Mastromatteo, Muzy survey Hawkes applications in high-frequency finance. ([arXiv][4])                                                    |
| Markovian LOB model           | Cont & de Larrard, “Price dynamics in a Markovian limit order market.” ([arXiv][5])                                                             |
| LOB simulation review         | “Limit Order Book Simulations: A Review.” Useful for positioning Janiform against LOB simulators. ([arXiv][6])                                  |
| Agent-based simulation        | ABIDES high-fidelity multi-agent market simulator. ([arXiv][7])                                                                                 |
| Neural LOB/event generators   | Event-based LOB simulation using Neural Hawkes processes. ([SSRN][8])                                                                           |
| NIG return modeling           | Barndorff-Nielsen’s NIG work for stock-return modeling. ([JSTOR][9])                                                                            |
| Affine transform pricing      | Duffie, Pan, Singleton, “Transform Analysis and Asset Pricing for Affine Jump-Diffusions.” ([SSRN][10])                                         |
| Fourier option pricing        | Carr & Madan, FFT option valuation from characteristic functions. ([NYU Tandon School of Engineering][11])                                      |
| Rough volatility / pricing    | El Euch & Rosenbaum, rough Heston characteristic function. ([arXiv][12])                                                                        |

The claim-to-evidence matrix lives in `design/paper/claims-evidence-matrix.md`.

[1]: https://rama.cont.perso.math.cnrs.fr/pdf/empirical.pdf "Empirical properties of asset returns: stylized facts and ..."
[2]: https://arxiv.org/html/2311.07738v2 "Revisiting Cont's Stylized Facts for Modern Stock Markets †"
[3]: https://www.jstor.org/stable/1913889 "A Subordinated Stochastic Process Model with Finite ..."
[4]: https://arxiv.org/abs/1502.04592 "Hawkes processes in finance"
[5]: https://arxiv.org/abs/1104.4596 "Price dynamics in a Markovian limit order market"
[6]: https://arxiv.org/html/2402.17359v1 "Limit Order Book Simulations: A Review"
[7]: https://arxiv.org/abs/1904.12066 "ABIDES: Towards High-Fidelity Market Simulation for AI Research"
[8]: https://papers.ssrn.com/sol3/Delivery.cfm/5152207.pdf?abstractid=5152207&mirid=1 "Event-Based Limit Order Book Simulation under a Neural ..."
[9]: https://www.jstor.org/stable/4616433 "Normal Inverse Gaussian Distributions and Stochastic ..."
[10]: https://papers.ssrn.com/sol3/papers.cfm?abstract_id=157733&utm_source=chatgpt.com "Transform Analysis and Asset Pricing for Affine Jump- ..."
[11]: https://engineering.nyu.edu/sites/default/files/2018-08/CarrMadan2_0.pdf "Option valuation using the fast Fourier transform"
[12]: https://arxiv.org/abs/1609.02108 "The characteristic function of rough Heston models"
