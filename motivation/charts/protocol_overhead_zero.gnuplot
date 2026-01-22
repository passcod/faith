set terminal png size 1200,800 font "sans,10" enhanced
set output 'charts/protocol_overhead_zero.png'
set title 'Fáith Protocol Overhead (Google Target - minus x1 baseline) (lower is better) (Y-axis from zero)'
set xlabel 'Number of Requests'
set ylabel 'Overhead Duration (ms)'
set yrange [0:*]
set grid ytics
set key outside right top
set style data histograms
set style histogram clustered gap 1
set style fill solid 0.8 border -1
set boxwidth 0.9
set offset 0,0,graph 0.15,0
set xtics ("10" 0, "100" 1)


plot 'charts/protocol_overhead_data.txt' using 2:xtic(1) title 'TCP', \
     '' using 3 title 'QUIC (Cubic)', \
     '' using 4 title 'QUIC (BBR)', \
     '' using ($0-0.27):2:(sprintf("%.0f",$2)) with labels center offset 0,1 font ",8" notitle, \
     '' using ($0):3:(sprintf("%.0f",$3)) with labels center offset 0,1 font ",8" notitle, \
     '' using ($0+0.27):4:(sprintf("%.0f",$4)) with labels center offset 0,1 font ",8" notitle
