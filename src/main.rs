// Assumes columns contain rank indices and each row is a respondant

use anyhow::{anyhow, Context, Result};
use clap::Parser;

/// Calculates the results of instant-runoff voting.
/// 
/// Pipe the contents of a CSV file (with headers) to use, where votes are contained in contiguous columns.
#[derive(Debug, Parser)]
struct Cli {
    /// What column ranks start at, indexed at 0.
    #[arg(short, long, default_value_t = 0)]
    start: usize,
    /// What value the ranks start at, i.e. what value corresponds to the highest rank.
    #[arg(short, long, default_value_t = 1)]
    indexed_at: usize,
    /// Outputs the winners only, delimited by newlines.
    #[arg(short, long)]
    raw: bool,
    /// The amount of columns which ranks occupy. If not specified, all remaining columns starting at the start index are used.
    len: Option<usize>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let votes = read_data(&cli)?;
    let results = votes.runoff();

    if cli.raw {
        for winner in results.map(|(winner, _, _)| winner) {
            println!("{winner}");
        }
    }
    else {
        for (i, (winner, counts, mut other)) in results.enumerate() {
            let cardinal = i + 1;

            println!("Winner #{cardinal}: {winner} with {counts} votes");

            other.sort_by(|(_, count_a), (_, count_b)| count_b.cmp(count_a));
            for (label, count) in other {
                println!("{label}: {count}");
            }

            println!();
            println!();
        }        
    }

    Ok(())
}

fn read_data(cli: &Cli) -> Result<Ballot<String>> {
    let mut csv_reader = csv::Reader::from_reader(std::io::stdin());

    let labels: Vec<_> = {
        let headers_start = csv_reader
            .headers()
            .context("headers issue")?
            .iter()
            .skip(cli.start)
            .map(String::from);

        if let Some(len) = cli.len {
            headers_start.take(len).collect()
        } else {
            headers_start.collect()
        }
    };
    let mut all_ranks = csv_reader.records().enumerate().try_fold(
        vec![],
        |mut all_ranks, (i, r)| {
            let row_ranks = {
                let cells: Vec<_> = {
                    let row = r.with_context(|| format!("bad record {i}"))?;
                    let cols_start = row.into_iter().skip(cli.start).map(String::from);

                    if let Some(len) = cli.len {
                        cols_start.take(len).collect()
                    } else {
                        cols_start.collect()
                    }
                };

                let merhaps: Result<Vec<_>, _> = cells
                    .into_iter()
                    .enumerate()
                    .map(|(j, v)| {
                        v.parse::<usize>()
                            .with_context(|| format!("invalid rank, record {i}, value {j}"))
                    })
                    .collect();

                merhaps?
            };

            let row_ranks_len = row_ranks.len();
            let labels_len = labels.len();
            
            if row_ranks_len != labels_len {
                Err(anyhow!("invalid number of ranks, record {i} (expected {labels_len}, got {row_ranks_len})"))
            } else {
                all_ranks.extend(row_ranks);

                Ok(all_ranks)
            }
        },
    )?;

    for value in all_ranks.iter_mut() {
        if let Some(sub) = value.checked_sub(cli.indexed_at) {
            *value = sub;
        } else {
            return Err(anyhow!(
                "bad index-at argument (ranks occur lower than the index)"
            ));
        }
    }

    let ballot = Ballot::new(labels, all_ranks).expect("labels and votes mismatch");

    Ok(ballot)
}

pub struct Ballot<T: Clone> {
    /// The names of the candidates
    labels: Vec<T>,
    /// The raw rankings. For all elements e in this vec, 0 <= e < width
    votes: Vec<usize>,
}

impl<T: Clone> Ballot<T> {
    pub fn new(labels: Vec<T>, votes: Vec<usize>) -> Result<Self, (Vec<T>, Vec<usize>)> {
        if votes.len() % labels.len() == 0 && votes.iter().copied().all(|v| v < labels.len()) {
            Ok(Self { labels, votes })
        } else {
            Err((labels, votes))
        }
    }

    fn count(&self) -> usize {
        self.labels.len()
    }

    fn rows(&mut self) -> impl Iterator<Item = &mut [usize]> + '_ {
        let count = self.count();

        self.votes.chunks_mut(count)
    }

    fn columns(&self) -> impl Iterator<Item = impl Iterator<Item = usize> + '_> + '_ {
        let count = self.count();

        (0..count).map(move |i| self.votes.iter().skip(i).step_by(count).copied())
    }

    fn remove_column(&mut self, col: usize) -> T {
        let count = self.count();

        for i in (0..self.votes.len()).rev().filter(|i| i % count == col) {
            self.votes.remove(i);
        }

        self.labels.remove(col)
    }

    /// Calculates each tier of an instant-runoff vote
    pub fn runoff(mut self) -> impl Iterator<Item = (T, usize, Vec<(T, usize)>)> {
        // According to R I G O R O U S testing (my head), this could
        // just be implemented by summing the ranks of votes that each
        // candidate gets, and then sorting the candidates according
        // to their vote counts.
        //
        // HOWEVER,
        //
        // that means each tier of votes cannot be counted i.e. only
        // the final result is known. Knowing the results of each
        // tier of vote makes it much easier to understand how the
        // results came to be.

        (0..self.count()).map(move |_| {
            let mut tier: Vec<_> = self
                .columns()
                .map(|col| col.filter(|vote_rank| *vote_rank == 0).count())
                .collect();

            let winner_index = (0..tier.len()).max_by_key(|i| tier[*i]).unwrap();

            for row in self.rows() {
                let winner_rank = row[winner_index];

                for choice in row.into_iter().filter(|rank| **rank > winner_rank) {
                    *choice -= 1;
                }
            }

            let winner_label = self.remove_column(winner_index);
            let winner_count = tier.remove(winner_index);
            let data: Vec<_> = self.labels.iter().cloned().zip(tier.into_iter()).collect();

            (winner_label, winner_count, data)
        })
    }
}

#[cfg(test)]
mod test {
    #[test]
    fn three_example() {
        let labels = vec![0, 1, 2];
        let values: Vec<_> = vec![
            [0, 1, 2],
            [0, 2, 1],
            [1, 2, 0],
            [1, 0, 2],
            [2, 0, 1],
            [2, 1, 0],
            [0, 2, 1],
            [0, 2, 1],
            [2, 0, 1],
        ]
        .into_iter()
        .flatten() // i put arrays and then flatten anyway so rustfmt doesn't put 50 billion numbers on one line
        .collect();
        let winners_known = vec![0, 2, 1]; // proven by the power of my hand and head

        let ballot = super::Ballot::new(labels, values).expect("label/values mismatch");
        let winners_exp: Vec<_> = ballot.runoff().map(|(winner, _, _)| winner).collect();

        assert_eq!(winners_known, winners_exp);
    }
}
